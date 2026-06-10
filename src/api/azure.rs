//! Real Azure DevOps REST backend (api-version 7.1).
//!
//! Implements [`WorkItemClient`] with synchronous (`reqwest::blocking`) calls:
//! WIQL for the assigned list, batch work-item GET (`$expand=relations`),
//! JSON-Patch field updates, and the comments API. Auth is whatever
//! `AuthState::header()` produced (Bearer for OAuth, Basic for a PAT).

use super::models::{Comment, DevLink, Iteration, WorkItem, WorkItemFilter, WorkItemState};
use super::WorkItemClient;
use anyhow::{anyhow, Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};

const API: &str = "api-version=7.1";
const COMMENTS_API: &str = "api-version=7.1-preview.4";
/// Display name of the Azure DevOps work-item field the Notes pane is bound to.
/// Its reference name varies per org (it's typically a custom field such as
/// `Custom.Notes`), so we resolve it at runtime from the fields metadata.
pub const NOTES_FIELD_NAME: &str = "Notes";
/// Last-resort reference name if the "Notes" field can't be resolved.
pub const NOTES_FIELD_FALLBACK: &str = "System.Notes";

pub struct AzureClient {
    http: reqwest::blocking::Client,
    /// e.g. https://dev.azure.com/contoso
    org_url: String,
    project: String,
    /// Team name, used to scope the iterations query. Empty → project default.
    team: String,
    auth_header: String,
    /// Cached reference name for the "Notes" field, resolved once from the
    /// fields metadata. Shared across clones (background workers) via `Arc`.
    notes_ref: Arc<OnceLock<String>>,
}

impl Clone for AzureClient {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            org_url: self.org_url.clone(),
            project: self.project.clone(),
            team: self.team.clone(),
            auth_header: self.auth_header.clone(),
            notes_ref: Arc::clone(&self.notes_ref),
        }
    }
}

impl AzureClient {
    pub fn new(
        org_url: impl Into<String>,
        project: impl Into<String>,
        team: impl Into<String>,
        auth_header: String,
    ) -> Self {
        let http = reqwest::blocking::Client::builder()
            .user_agent("lazyaz")
            .build()
            .unwrap_or_default();
        Self {
            http,
            org_url: org_url.into().trim_end_matches('/').to_string(),
            project: project.into(),
            team: team.into(),
            auth_header,
            notes_ref: Arc::new(OnceLock::new()),
        }
    }

    fn get_json(&self, url: &str) -> Result<Value> {
        self.http
            .get(url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("GET {url}"))?
            .json()
            .context("parsing response")
    }

    /// Reference name of the field whose display name is "Notes", resolved once
    /// from the project's fields metadata and cached for the process lifetime.
    fn notes_field_ref(&self) -> &str {
        self.notes_ref.get_or_init(|| self.resolve_notes_ref())
    }

    fn resolve_notes_ref(&self) -> String {
        let url = format!("{}/{}/_apis/wit/fields?{API}", self.org_url, self.project);
        if let Ok(v) = self.get_json(&url)
            && let Some(arr) = v["value"].as_array()
        {
            for f in arr {
                let matches = f["name"]
                    .as_str()
                    .map(|n| n.eq_ignore_ascii_case(NOTES_FIELD_NAME))
                    .unwrap_or(false);
                if matches && let Some(rn) = f["referenceName"].as_str() {
                    return rn.to_string();
                }
            }
        }
        NOTES_FIELD_FALLBACK.to_string()
    }

    /// Field reference name for one of our logical field keys.
    fn field_ref(&self, key: &str) -> Option<String> {
        Some(match key {
            "title" => "System.Title".to_string(),
            "state" => "System.State".to_string(),
            "description" => "System.Description".to_string(),
            "assignee" => "System.AssignedTo".to_string(),
            "iteration" => "System.IterationPath".to_string(),
            "tags" => "System.Tags".to_string(),
            "notes" => self.notes_field_ref().to_string(),
            _ => return None,
        })
    }
}

impl WorkItemClient for AzureClient {
    fn list_assigned(&self, filter: &WorkItemFilter) -> Result<Vec<WorkItem>> {
        // WIQL returns only ids; we then batch-fetch the work items. Build the
        // WHERE clause from the timeframe + iteration filter so the server does
        // the filtering (rather than fetching everything and trimming locally).
        let mut where_clauses = vec!["[System.AssignedTo] = @Me".to_string()];
        if let Some(tf) = filter.timeframe.wiql_clause() {
            where_clauses.push(tf);
        }
        if !filter.iterations.is_empty() {
            let ors = filter
                .iterations
                .iter()
                .map(|p| format!("[System.IterationPath] UNDER '{}'", p.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(" OR ");
            where_clauses.push(format!("({ors})"));
        }
        if !filter.item_types.is_empty() {
            let list = filter
                .item_types
                .iter()
                .map(|t| format!("'{}'", t.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(", ");
            where_clauses.push(format!("[System.WorkItemType] IN ({list})"));
        }
        let query = format!(
            "SELECT [System.Id] FROM WorkItems WHERE {} ORDER BY [System.ChangedDate] DESC",
            where_clauses.join(" AND ")
        );
        let wiql = json!({ "query": query });
        let url = format!("{}/{}/_apis/wit/wiql?{API}", self.org_url, self.project);
        let resp: Value = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .json(&wiql)
            .send()
            .context("WIQL query")?
            .error_for_status()
            .context("WIQL query")?
            .json()
            .context("parsing WIQL response")?;

        let ids: Vec<u64> = resp["workItems"]
            .as_array()
            .map(|a| a.iter().filter_map(|w| w["id"].as_u64()).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        // Azure caps batch GET at 200 ids.
        let ids_csv = ids
            .iter()
            .take(200)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let url = format!(
            "{}/_apis/wit/workitems?ids={ids_csv}&$expand=relations&{API}",
            self.org_url
        );
        let batch = self.get_json(&url)?;
        let items: Vec<WorkItem> = batch["value"]
            .as_array()
            .map(|a| a.iter().map(|v| parse_work_item(v, self.notes_field_ref())).collect())
            .unwrap_or_default();
        Ok(items)
    }

    fn list_iterations(&self) -> Vec<Iteration> {
        // The team iterations endpoint; omit the team segment to use the
        // project's default team when none is configured.
        let scope = if self.team.trim().is_empty() {
            self.project.clone()
        } else {
            format!("{}/{}", self.project, self.team.replace(' ', "%20"))
        };
        let url = format!(
            "{}/{scope}/_apis/work/teamsettings/iterations?{API}",
            self.org_url
        );
        let Ok(v) = self.get_json(&url) else {
            return Vec::new();
        };
        v["value"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|it| {
                        let path = it["path"].as_str()?.to_string();
                        let name = it["name"].as_str().unwrap_or(&path).to_string();
                        let is_current =
                            it["attributes"]["timeFrame"].as_str() == Some("current");
                        Some(Iteration { path, name, is_current })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn get(&self, id: u32) -> Result<WorkItem> {
        let url = format!(
            "{}/_apis/wit/workitems/{id}?$expand=relations&{API}",
            self.org_url
        );
        let mut item = parse_work_item(&self.get_json(&url)?, self.notes_field_ref());
        // Valid workflow states for this item's type, for the state picker.
        item.available_states = self.list_states(&item.item_type);
        // Comments come from a separate endpoint.
        let curl = format!(
            "{}/{}/_apis/wit/workItems/{id}/comments?{COMMENTS_API}",
            self.org_url, self.project
        );
        if let Ok(c) = self.get_json(&curl)
            && let Some(arr) = c["comments"].as_array() {
                item.comments = arr.iter().map(parse_comment).collect();
            }
        Ok(item)
    }

    fn add_comment(&mut self, id: u32, _author: &str, text: &str) -> Result<()> {
        let url = format!(
            "{}/{}/_apis/wit/workItems/{id}/comments?{COMMENTS_API}",
            self.org_url, self.project
        );
        self.http
            .post(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .json(&json!({ "text": text }))
            .send()
            .context("adding comment")?
            .error_for_status()
            .context("adding comment")?;
        Ok(())
    }

    fn update_comment(&mut self, id: u32, comment_id: u32, text: &str) -> Result<()> {
        let url = format!(
            "{}/{}/_apis/wit/workItems/{id}/comments/{comment_id}?{COMMENTS_API}",
            self.org_url, self.project
        );
        self.http
            .patch(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .json(&json!({ "text": text }))
            .send()
            .context("editing comment")?
            .error_for_status()
            .context("editing comment")?;
        Ok(())
    }

    fn delete_comment(&mut self, id: u32, comment_id: u32) -> Result<()> {
        let url = format!(
            "{}/{}/_apis/wit/workItems/{id}/comments/{comment_id}?{COMMENTS_API}",
            self.org_url, self.project
        );
        self.http
            .delete(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .context("deleting comment")?
            .error_for_status()
            .context("deleting comment")?;
        Ok(())
    }

    fn update_field(&mut self, id: u32, field: &str, value: &str) -> Result<()> {
        let reference = self
            .field_ref(field)
            .ok_or_else(|| anyhow!("field '{field}' is not supported"))?;
        // Azure DevOps tags are semicolon-separated.
        let value = if field == "tags" {
            value.replace(',', ";")
        } else {
            value.to_string()
        };
        let patch = json!([{ "op": "add", "path": format!("/fields/{reference}"), "value": value }]);
        let url = format!("{}/_apis/wit/workitems/{id}?{API}", self.org_url);
        self.http
            .patch(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json-patch+json")
            .body(serde_json::to_string(&patch)?)
            .send()
            .context("updating field")?
            .error_for_status()
            .context("updating field")?;
        Ok(())
    }

    fn list_projects(&self, org: &str) -> Vec<String> {
        // During first-run setup the client's `org_url` isn't set yet, so the
        // base comes from the org the user just picked in the wizard.
        let base = org_base_url(org, &self.org_url);
        let url = format!("{base}/_apis/projects?{API}");
        self.get_json(&url)
            .ok()
            .and_then(|v| {
                v["value"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|p| p["name"].as_str().map(String::from))
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    fn reconfigure(&mut self, org_url: &str, project: &str) {
        self.org_url = org_url.trim_end_matches('/').to_string();
        self.project = project.to_string();
    }

    fn list_tags(&self) -> Vec<String> {
        let url = format!("{}/{}/_apis/wit/tags?{API}", self.org_url, self.project);
        self.get_json(&url)
            .ok()
            .and_then(|v| {
                v["value"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|t| t["name"].as_str().map(String::from))
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    fn list_states(&self, item_type: &str) -> Vec<String> {
        if item_type.is_empty() {
            return Vec::new();
        }
        let encoded = item_type.replace(' ', "%20");
        let url = format!(
            "{}/{}/_apis/wit/workitemtypes/{encoded}/states?{API}",
            self.org_url, self.project
        );
        self.get_json(&url)
            .ok()
            .and_then(|v| {
                v["value"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|s| s["name"].as_str().map(String::from))
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn WorkItemClient + Send + Sync> {
        Box::new(self.clone())
    }
}

/// Resolve the REST base URL for an organization. Accepts either a bare org
/// name (e.g. `contoso`) or a full URL, falling back to `fallback` when empty.
fn org_base_url(org: &str, fallback: &str) -> String {
    let org = org.trim();
    if org.is_empty() {
        return fallback.trim_end_matches('/').to_string();
    }
    if org.starts_with("http://") || org.starts_with("https://") {
        org.trim_end_matches('/').to_string()
    } else {
        format!("https://dev.azure.com/{org}")
    }
}

fn field_str(fields: &Value, key: &str) -> String {
    fields[key].as_str().unwrap_or("").to_string()
}

/// `System.AssignedTo` is either a string or an identity object.
fn identity(fields: &Value, key: &str) -> String {
    let v = &fields[key];
    v["displayName"]
        .as_str()
        .or_else(|| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn parse_work_item(v: &Value, notes_field: &str) -> WorkItem {
    let fields = &v["fields"];
    let id = v["id"].as_u64().unwrap_or(0) as u32;
    let rev = v["rev"].as_u64().unwrap_or(1) as u32;

    let tags = field_str(fields, "System.Tags");
    let tags = tags
        .split(';')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    let mut parent = None;
    let mut children = Vec::new();
    let mut dev_links = Vec::new();
    if let Some(rels) = v["relations"].as_array() {
        for r in rels {
            let rel = r["rel"].as_str().unwrap_or("");
            let url = r["url"].as_str().unwrap_or("");
            let linked = url.rsplit('/').next().and_then(|s| s.parse::<u32>().ok());
            match (rel, linked) {
                ("System.LinkTypes.Hierarchy-Reverse", Some(p)) => parent = Some(p),
                ("System.LinkTypes.Hierarchy-Forward", Some(c)) => children.push(c),
                ("ArtifactLink", _) => {
                    let name = r["attributes"]["name"].as_str().unwrap_or("Link");
                    dev_links.push(DevLink {
                        kind: name.to_string(),
                        name: r["attributes"]["comment"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .unwrap_or(name)
                            .to_string(),
                        url: url.to_string(),
                    });
                }
                ("Hyperlink", _) => {
                    let label = r["attributes"]["comment"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(url);
                    dev_links.push(DevLink {
                        kind: "Hyperlink".to_string(),
                        name: label.to_string(),
                        url: url.to_string(),
                    });
                }
                _ => {}
            }
        }
    }

    let state_name = field_str(fields, "System.State");
    let state = WorkItemState::from_label(&state_name)
        .unwrap_or_else(|| WorkItemState::category(&state_name));

    WorkItem {
        id,
        title: field_str(fields, "System.Title"),
        item_type: field_str(fields, "System.WorkItemType"),
        state,
        state_name,
        available_states: Vec::new(),
        assigned_to: identity(fields, "System.AssignedTo"),
        iteration: field_str(fields, "System.IterationPath"),
        description: strip_html(&field_str(fields, "System.Description")),
        notes: strip_html(&field_str(fields, notes_field)),
        tags,
        parent,
        children,
        dev_links,
        comments: Vec::new(),
        changed_days_ago: days_since(&field_str(fields, "System.ChangedDate")),
        rev,
    }
}

fn parse_comment(v: &Value) -> Comment {
    Comment {
        id: v["id"].as_u64().unwrap_or(0) as u32,
        author: v["createdBy"]["displayName"].as_str().unwrap_or("").to_string(),
        when: v["createdDate"].as_str().unwrap_or("").to_string(),
        text: strip_html(v["text"].as_str().unwrap_or("")),
    }
}

/// Very small HTML-to-text: drops tags and decodes a few entities. Azure
/// returns rich-text fields as HTML; the TUI only renders plain text.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .trim()
        .to_string()
}

/// Whole days between an ISO-8601 timestamp (`YYYY-MM-DD...`) and today (UTC).
fn days_since(iso: &str) -> u32 {
    let parse = || -> Option<i64> {
        let date = iso.get(0..10)?; // YYYY-MM-DD
        let mut p = date.split('-');
        let y: i64 = p.next()?.parse().ok()?;
        let m: i64 = p.next()?.parse().ok()?;
        let d: i64 = p.next()?.parse().ok()?;
        Some(days_from_civil(y, m, d))
    };
    let Some(then) = parse() else { return 0 };
    let now_days = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        / 86_400) as i64;
    (now_days - then).max(0) as u32
}

/// Days since the Unix epoch for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
