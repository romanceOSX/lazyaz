use super::models::{Comment, DevLink, Iteration, WorkItem, WorkItemFilter, WorkItemState};
use super::WorkItemClient;
use anyhow::{anyhow, Result};
use std::sync::{Arc, Mutex};

/// In-memory backend with a small canned parent/child graph. State lives behind
/// an `Arc<Mutex<…>>` so cloned clients (e.g. background workers) share a single
/// "server" copy, mirroring how the real REST backend behaves.
#[derive(Clone)]
pub struct MockClient {
    items: Arc<Mutex<Vec<WorkItem>>>,
}

impl Default for MockClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClient {
    pub fn new() -> Self {
        let me = "you@example.com";
        let mk = |id, title: &str, item_type: &str, state: WorkItemState, parent, children: Vec<u32>, days, desc: &str, notes: &str, tags: Vec<&str>, dev_links: Vec<DevLink>, comments| WorkItem {
            id,
            title: title.into(),
            item_type: item_type.into(),
            state,
            state_name: state.label().to_string(),
            available_states: vec![
                "New".into(),
                "Active".into(),
                "Resolved".into(),
                "Closed".into(),
            ],
            assigned_to: me.into(),
            iteration: "Sprint 24".into(),
            description: desc.into(),
            notes: notes.into(),
            tags: tags.into_iter().map(String::from).collect(),
            parent,
            children,
            dev_links,
            comments,
            changed_days_ago: days,
            rev: 1,
        };
        let gh = |name: &str, url: &str| DevLink {
            kind: "GitHub".into(),
            name: name.into(),
            url: url.into(),
        };
        let items = vec![
            mk(1001, "Epic: Terminal-first Azure DevOps workflow", "Epic", WorkItemState::Active, None, vec![1002, 1003], 2,
                "Umbrella for the lazyaz TUI initiative.", "Stakeholders: DX guild.", vec!["tui", "dx"], vec![],
                vec![Comment { id: 1, author: "pm@example.com".into(), when: "2d ago".into(), text: "Let's prioritize read flows first.".into() }]),
            mk(1002, "Browse my assigned user stories from the terminal", "User Story", WorkItemState::Active, Some(1001), vec![1004, 1005], 0,
                "As a developer I want to list and open my stories.", "Acceptance: filter by timeframe.", vec!["read"],
                vec![gh("PR #42: list view", "https://github.com/romanceOSX/lazyaz/pull/42")], vec![]),
            mk(1003, "Edit work items in $EDITOR and sync back", "User Story", WorkItemState::New, Some(1001), vec![], 6,
                "As a developer I want to edit fields in my editor.", "Watch out for the suspend/restore dance.", vec!["write", "stretch"],
                vec![gh("commit a1b2c3d", "https://github.com/romanceOSX/lazyaz/commit/a1b2c3d")],
                vec![Comment { id: 1, author: "you@example.com".into(), when: "6d ago".into(), text: "Needs terminal suspend.".into() }]),
            mk(1004, "Wire WIQL query for assigned items", "Task", WorkItemState::Active, Some(1002), vec![], 1,
                "Build the @Me WIQL and map the response.", "", vec![], vec![], vec![]),
            mk(1005, "Render detail pane with comments", "Task", WorkItemState::Closed, Some(1002), vec![], 12,
                "Lay out fields + comments + relations.", "Done in sprint 23.", vec!["done"], vec![], vec![]),
        ];
        Self {
            items: Arc::new(Mutex::new(items)),
        }
    }
}

impl WorkItemClient for MockClient {
    fn list_assigned(&self, filter: &WorkItemFilter) -> Result<Vec<WorkItem>> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|w| filter.matches(w)).cloned().collect())
    }

    fn list_iterations(&self) -> Vec<Iteration> {
        // Canned sprints; the mock items all live in "Sprint 24" (the current).
        vec![
            Iteration { path: "Sprint 23".into(), name: "Sprint 23".into(), is_current: false },
            Iteration { path: "Sprint 24".into(), name: "Sprint 24".into(), is_current: true },
            Iteration { path: "Sprint 25".into(), name: "Sprint 25".into(), is_current: false },
        ]
    }

    fn get(&self, id: u32) -> Result<WorkItem> {
        let items = self.items.lock().unwrap();
        items
            .iter()
            .find(|w| w.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("work item {id} not found"))
    }

    fn add_comment(&mut self, id: u32, author: &str, text: &str) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let i = items
            .iter()
            .position(|w| w.id == id)
            .ok_or_else(|| anyhow!("work item {id} not found"))?;
        let next_id = items[i].comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        items[i].comments.push(Comment {
            id: next_id,
            author: author.to_string(),
            when: "just now".into(),
            text: text.to_string(),
        });
        items[i].changed_days_ago = 0;
        items[i].rev += 1;
        Ok(())
    }

    fn update_comment(&mut self, id: u32, comment_id: u32, text: &str) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let i = items
            .iter()
            .position(|w| w.id == id)
            .ok_or_else(|| anyhow!("work item {id} not found"))?;
        let c = items[i]
            .comments
            .iter_mut()
            .find(|c| c.id == comment_id)
            .ok_or_else(|| anyhow!("comment {comment_id} not found on #{id}"))?;
        c.text = text.to_string();
        c.when = "just now (edited)".into();
        items[i].changed_days_ago = 0;
        items[i].rev += 1;
        Ok(())
    }

    fn delete_comment(&mut self, id: u32, comment_id: u32) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let i = items
            .iter()
            .position(|w| w.id == id)
            .ok_or_else(|| anyhow!("work item {id} not found"))?;
        let before = items[i].comments.len();
        items[i].comments.retain(|c| c.id != comment_id);
        if items[i].comments.len() == before {
            return Err(anyhow!("comment {comment_id} not found on #{id}"));
        }
        items[i].changed_days_ago = 0;
        items[i].rev += 1;
        Ok(())
    }

    fn update_field(&mut self, id: u32, field: &str, value: &str) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let i = items
            .iter()
            .position(|w| w.id == id)
            .ok_or_else(|| anyhow!("work item {id} not found"))?;
        match field {
            "title" => items[i].title = value.to_string(),
            "description" => items[i].description = value.to_string(),
            "notes" => items[i].notes = value.to_string(),
            "iteration" => items[i].iteration = value.to_string(),
            "assignee" => items[i].assigned_to = value.to_string(),
            "tags" => {
                items[i].tags = value
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            }
            "state" => {
                items[i].state = WorkItemState::from_label(value)
                    .unwrap_or_else(|| WorkItemState::category(value));
                items[i].state_name = value.to_string();
            }
            other => return Err(anyhow!("field '{other}' is not editable in the skeleton")),
        }
        items[i].changed_days_ago = 0;
        items[i].rev += 1;
        Ok(())
    }

    fn simulate_remote_edit(&mut self, id: u32) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let i = items
            .iter()
            .position(|w| w.id == id)
            .ok_or_else(|| anyhow!("work item {id} not found"))?;
        let item = &mut items[i];
        item.description = format!("[edited by teammate] {}", item.description);
        item.rev += 1;
        item.changed_days_ago = 0;
        Ok(())
    }

    fn list_projects(&self, _org: &str) -> Vec<String> {
        // Canned; the real client will call the Projects REST API.
        vec![
            "Widgets".into(),
            "Platform".into(),
            "Mobile App".into(),
            "Data Pipeline".into(),
            "Infrastructure".into(),
        ]
    }

    fn list_tags(&self) -> Vec<String> {
        let items = self.items.lock().unwrap();
        let mut tags: Vec<String> = items
            .iter()
            .flat_map(|w| w.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    fn list_states(&self, _item_type: &str) -> Vec<String> {
        vec![
            "New".into(),
            "Active".into(),
            "Resolved".into(),
            "Closed".into(),
        ]
    }

    fn clone_box(&self) -> Box<dyn WorkItemClient + Send + Sync> {
        Box::new(self.clone())
    }
}
