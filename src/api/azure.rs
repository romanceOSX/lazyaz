//! Real Azure DevOps REST backend (api-version 7.1).
//!
//! Implements [`WorkItemClient`] with synchronous (`reqwest::blocking`) calls:
//! WIQL for the assigned list, batch work-item GET (`$expand=relations`),
//! JSON-Patch field updates, and the comments API. Auth is whatever
//! `AuthState::header()` produced (Bearer for OAuth, Basic for a PAT).

use super::models::{Comment, Timeframe, WorkItem, WorkItemState};
use super::WorkItemClient;
use anyhow::{anyhow, Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};

const API: &str = "api-version=7.1";
const COMMENTS_API: &str = "api-version=7.1-preview.4";
/// There is no standard "Notes" field; we map it to Acceptance Criteria, which
/// most work-item types carry. Change here if your process uses another field.
const NOTES_FIELD: &str = "Microsoft.VSTS.Common.AcceptanceCriteria";

pub struct AzureClient {
    http: reqwest::blocking::Client,
    /// e.g. https://dev.azure.com/contoso
    org_url: String,
    project: String,
    auth_header: String,
}

impl AzureClient {
    pub fn new(org_url: impl Into<String>, project: impl Into<String>, auth_header: String) -> Self {
        let http = reqwest::blocking::Client::builder()
            .user_agent("lazyaz")
            .build()
            .unwrap_or_default();
        Self {
            http,
            org_url: org_url.into().trim_end_matches('/').to_string(),
            project: project.into(),
            auth_header,
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

    /// Field reference name for one of our logical field keys.
    fn field_ref(key: &str) -> Option<&'static str> {
        Some(match key {
            "title" => "System.Title",
            "state" => "System.State",
            "description" => "System.Description",
            "assignee" => "System.AssignedTo",
            "iteration" => "System.IterationPath",
            "tags" => "System.Tags",
            "notes" => NOTES_FIELD,
            _ => return None,
        })
    }
}

impl WorkItemClient for AzureClient {
    fn list_assigned(&self, timeframe: Timeframe) -> Result<Vec<WorkItem>> {
        // WIQL returns only ids; we then batch-fetch the work items.
        let wiql = json!({
            "query": "SELECT [System.Id] FROM WorkItems \
                      WHERE [System.AssignedTo] = @Me \
                      ORDER BY [System.ChangedDate] DESC"
        });
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
        let max_days = timeframe.max_days();
        let items: Vec<WorkItem> = batch["value"]
            .as_array()
            .map(|a| a.iter().map(parse_work_item).collect())
            .unwrap_or_default();
        Ok(items
            .into_iter()
            .filter(|w| w.changed_days_ago <= max_days)
            .collect())
    }

    fn get(&self, id: u32) -> Result<WorkItem> {
        let url = format!(
            "{}/_apis/wit/workitems/{id}?$expand=relations&{API}",
            self.org_url
        );
        let mut item = parse_work_item(&self.get_json(&url)?);
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

    fn update_field(&mut self, id: u32, field: &str, value: &str) -> Result<()> {
        let reference = Self::field_ref(field)
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

    fn list_projects(&self, _org: &str) -> Vec<String> {
        let url = format!("{}/_apis/projects?{API}", self.org_url);
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

fn parse_work_item(v: &Value) -> WorkItem {
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
    if let Some(rels) = v["relations"].as_array() {
        for r in rels {
            let rel = r["rel"].as_str().unwrap_or("");
            let url = r["url"].as_str().unwrap_or("");
            let linked = url.rsplit('/').next().and_then(|s| s.parse::<u32>().ok());
            match (rel, linked) {
                ("System.LinkTypes.Hierarchy-Reverse", Some(p)) => parent = Some(p),
                ("System.LinkTypes.Hierarchy-Forward", Some(c)) => children.push(c),
                _ => {}
            }
        }
    }

    WorkItem {
        id,
        title: field_str(fields, "System.Title"),
        item_type: field_str(fields, "System.WorkItemType"),
        state: WorkItemState::from_label(&field_str(fields, "System.State"))
            .unwrap_or(WorkItemState::New),
        assigned_to: identity(fields, "System.AssignedTo"),
        iteration: field_str(fields, "System.IterationPath"),
        description: strip_html(&field_str(fields, "System.Description")),
        notes: strip_html(&field_str(fields, NOTES_FIELD)),
        tags,
        parent,
        children,
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
