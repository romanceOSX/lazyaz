use super::models::{Comment, Timeframe, WorkItem, WorkItemState};
use super::WorkItemClient;
use anyhow::{anyhow, Result};

/// In-memory backend with a small canned parent/child graph.
pub struct MockClient {
    items: Vec<WorkItem>,
}

impl Default for MockClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClient {
    pub fn new() -> Self {
        let me = "you@example.com";
        let mk = |id, title: &str, item_type: &str, state, parent, children: Vec<u32>, days, desc: &str, notes: &str, tags: Vec<&str>, comments| WorkItem {
            id,
            title: title.into(),
            item_type: item_type.into(),
            state,
            assigned_to: me.into(),
            iteration: "Sprint 24".into(),
            description: desc.into(),
            notes: notes.into(),
            tags: tags.into_iter().map(String::from).collect(),
            parent,
            children,
            comments,
            changed_days_ago: days,
            rev: 1,
        };
        let items = vec![
            mk(1001, "Epic: Terminal-first Azure DevOps workflow", "Epic", WorkItemState::Active, None, vec![1002, 1003], 2,
                "Umbrella for the lazyaz TUI initiative.", "Stakeholders: DX guild.", vec!["tui", "dx"],
                vec![Comment { id: 1, author: "pm@example.com".into(), when: "2d ago".into(), text: "Let's prioritize read flows first.".into() }]),
            mk(1002, "Browse my assigned user stories from the terminal", "User Story", WorkItemState::Active, Some(1001), vec![1004, 1005], 0,
                "As a developer I want to list and open my stories.", "Acceptance: filter by timeframe.", vec!["read"], vec![]),
            mk(1003, "Edit work items in $EDITOR and sync back", "User Story", WorkItemState::New, Some(1001), vec![], 6,
                "As a developer I want to edit fields in my editor.", "Watch out for the suspend/restore dance.", vec!["write", "stretch"],
                vec![Comment { id: 1, author: "you@example.com".into(), when: "6d ago".into(), text: "Needs terminal suspend.".into() }]),
            mk(1004, "Wire WIQL query for assigned items", "Task", WorkItemState::Active, Some(1002), vec![], 1,
                "Build the @Me WIQL and map the response.", "", vec![], vec![]),
            mk(1005, "Render detail pane with comments", "Task", WorkItemState::Closed, Some(1002), vec![], 12,
                "Lay out fields + comments + relations.", "Done in sprint 23.", vec!["done"], vec![]),
        ];
        Self { items }
    }

    fn index_of(&self, id: u32) -> Option<usize> {
        self.items.iter().position(|w| w.id == id)
    }

}

impl WorkItemClient for MockClient {
    fn list_assigned(&self, timeframe: Timeframe) -> Result<Vec<WorkItem>> {
        let max = timeframe.max_days();
        Ok(self
            .items
            .iter()
            .filter(|w| w.changed_days_ago <= max)
            .cloned()
            .collect())
    }

    fn get(&self, id: u32) -> Result<WorkItem> {
        self.index_of(id)
            .map(|i| self.items[i].clone())
            .ok_or_else(|| anyhow!("work item {id} not found"))
    }

    fn add_comment(&mut self, id: u32, author: &str, text: &str) -> Result<()> {
        let i = self.index_of(id).ok_or_else(|| anyhow!("work item {id} not found"))?;
        let next_id = self.items[i].comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        self.items[i].comments.push(Comment {
            id: next_id,
            author: author.to_string(),
            when: "just now".into(),
            text: text.to_string(),
        });
        self.items[i].changed_days_ago = 0;
        self.items[i].rev += 1;
        Ok(())
    }

    fn update_comment(&mut self, id: u32, comment_id: u32, text: &str) -> Result<()> {
        let i = self.index_of(id).ok_or_else(|| anyhow!("work item {id} not found"))?;
        let c = self.items[i]
            .comments
            .iter_mut()
            .find(|c| c.id == comment_id)
            .ok_or_else(|| anyhow!("comment {comment_id} not found on #{id}"))?;
        c.text = text.to_string();
        c.when = "just now (edited)".into();
        self.items[i].changed_days_ago = 0;
        self.items[i].rev += 1;
        Ok(())
    }

    fn update_field(&mut self, id: u32, field: &str, value: &str) -> Result<()> {
        let i = self.index_of(id).ok_or_else(|| anyhow!("work item {id} not found"))?;
        match field {
            "title" => self.items[i].title = value.to_string(),
            "description" => self.items[i].description = value.to_string(),
            "notes" => self.items[i].notes = value.to_string(),
            "iteration" => self.items[i].iteration = value.to_string(),
            "assignee" => self.items[i].assigned_to = value.to_string(),
            "tags" => {
                self.items[i].tags = value
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            }
            "state" => {
                self.items[i].state = WorkItemState::from_label(value)
                    .ok_or_else(|| anyhow!("invalid state '{value}'"))?;
            }
            other => return Err(anyhow!("field '{other}' is not editable in the skeleton")),
        }
        self.items[i].changed_days_ago = 0;
        self.items[i].rev += 1;
        Ok(())
    }

    fn simulate_remote_edit(&mut self, id: u32) -> Result<()> {
        let i = self.index_of(id).ok_or_else(|| anyhow!("work item {id} not found"))?;
        let item = &mut self.items[i];
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
}
