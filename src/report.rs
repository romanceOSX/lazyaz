//! Self-contained HTML report of a work-item tree and its dependencies.
//!
//! [`render_tree_html`] turns a cache of [`WorkItem`]s (keyed by id) into a
//! single static HTML document with two views of the subtree rooted at a focus
//! item: a collapsible nested hierarchy (works fully offline) and an embedded
//! Mermaid dependency graph (parent → child edges, loaded from a CDN), plus a
//! story-points statistics block (totals, done/active/to-do burn-down and a
//! per-type breakdown). The output has no external assets beyond the optional
//! Mermaid script, so it can be written to a temp file and opened in a browser.

use crate::api::models::WorkItem;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Aggregated story-point statistics over a set of work items.
#[derive(Default)]
struct PointStats {
    total: f64,
    done: f64,
    active: f64,
    todo: f64,
    estimated: usize,
    unestimated: usize,
    /// item type → (points, count)
    by_type: BTreeMap<String, (f64, usize)>,
}

impl PointStats {
    fn collect<'a>(items: impl Iterator<Item = &'a WorkItem>) -> Self {
        let mut s = PointStats::default();
        for w in items {
            let entry = s.by_type.entry(w.item_type.clone()).or_default();
            entry.1 += 1;
            match w.story_points {
                Some(p) if p > 0.0 => {
                    s.total += p;
                    s.estimated += 1;
                    entry.0 += p;
                    match state_class(&w.state_name) {
                        "done" => s.done += p,
                        "active" => s.active += p,
                        // "removed" points are excluded from the burn-down split
                        "removed" => {}
                        _ => s.todo += p,
                    }
                }
                _ => s.unestimated += 1,
            }
        }
        s
    }
}

/// Format a points value without a trailing `.0` (e.g. `8` not `8.0`, `2.5` kept).
fn fmt_pts(p: f64) -> String {
    if (p.fract()).abs() < f64::EPSILON {
        format!("{}", p as i64)
    } else {
        format!("{p:.1}")
    }
}


/// Escape text for safe inclusion in HTML element content / attributes.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Sanitise a string for use inside a (quoted) Mermaid node label: collapse
/// whitespace and replace characters that would break the diagram syntax.
fn mermaid_label(s: &str) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .replace('"', "'")
        .replace(['[', ']', '{', '}', '(', ')', '<', '>', '|', '#'], " ")
}

/// CSS class suffix for a state, so the report can colour nodes by status.
fn state_class(state_name: &str) -> &'static str {
    let s = state_name.to_ascii_lowercase();
    if s.contains("done") || s.contains("closed") || s.contains("resolved") || s.contains("completed") {
        "done"
    } else if s.contains("active") || s.contains("progress") || s.contains("committed") || s.contains("doing") {
        "active"
    } else if s.contains("removed") || s.contains("cut") {
        "removed"
    } else {
        "new"
    }
}

/// Build the full HTML document. `url_prefix`, when set, is prepended to an id
/// to link each item out to the Azure DevOps web UI (`{prefix}{id}`).
pub fn render_tree_html(
    root: u32,
    cache: &HashMap<u32, WorkItem>,
    title: &str,
    url_prefix: Option<&str>,
) -> String {
    let mut hierarchy = String::new();
    let mut visited = HashSet::new();
    render_node(root, cache, url_prefix, &mut visited, &mut hierarchy);

    let mut nodes = String::new();
    let mut edges = String::new();
    let mut seen = HashSet::new();
    build_graph(root, cache, &mut seen, &mut nodes, &mut edges);

    let total = seen.len();
    let stats = PointStats::collect(seen.iter().filter_map(|id| cache.get(id)));
    let stats_html = render_stats(&stats);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  :root {{ color-scheme: light dark; }}
  body {{ font-family: -apple-system, Segoe UI, Roboto, Helvetica, Arial, sans-serif;
         margin: 0; padding: 1.5rem 2rem; line-height: 1.5; }}
  h1 {{ font-size: 1.4rem; margin: 0 0 .25rem; }}
  .meta {{ color: #888; font-size: .85rem; margin-bottom: 1.5rem; }}
  .cols {{ display: flex; gap: 2rem; align-items: flex-start; flex-wrap: wrap; }}
  .col {{ flex: 1 1 420px; min-width: 320px; }}
  h2 {{ font-size: 1rem; text-transform: uppercase; letter-spacing: .05em; color: #888;
        border-bottom: 1px solid #8884; padding-bottom: .3rem; }}
  ul {{ list-style: none; padding-left: 1.1rem; margin: .2rem 0; }}
  li {{ margin: .15rem 0; }}
  details > summary {{ cursor: pointer; }}
  .leaf {{ padding-left: 1.1rem; }}
  .id {{ font-variant-numeric: tabular-nums; color: #888; }}
  a {{ text-decoration: none; }} a:hover {{ text-decoration: underline; }}
  .type {{ font-size: .72rem; padding: .05rem .4rem; border-radius: .6rem;
           background: #8883; margin: 0 .35rem; white-space: nowrap; }}
  .state {{ font-size: .72rem; padding: .05rem .4rem; border-radius: .6rem; margin-left: .35rem; }}
  .state.new {{ background:#9993; color:#bbb; }}
  .state.active {{ background:#2d8cff33; color:#4ea0ff; }}
  .state.done {{ background:#27ae6033; color:#2ecc71; }}
  .state.removed {{ background:#e74c3c33; color:#e74c3c; }}
  .who {{ color:#888; font-size:.8rem; margin-left:.35rem; }}
  .mermaid {{ background: transparent; }}
  .stats {{ margin-bottom: 1.5rem; }}
  .cards {{ display:flex; gap:1rem; flex-wrap:wrap; margin:.5rem 0 1rem; }}
  .card {{ background:#8881; border-radius:.6rem; padding:.6rem 1rem; min-width:6rem; }}
  .card .n {{ font-size:1.6rem; font-weight:600; font-variant-numeric:tabular-nums; }}
  .card .l {{ font-size:.72rem; text-transform:uppercase; letter-spacing:.05em; color:#888; }}
  .bar {{ display:flex; height:1.1rem; border-radius:.55rem; overflow:hidden; max-width:640px;
          background:#8882; font-size:.7rem; }}
  .bar > span {{ display:flex; align-items:center; justify-content:center; color:#0008; white-space:nowrap; }}
  .bar .b-done {{ background:#2ecc71; }}
  .bar .b-active {{ background:#4ea0ff; }}
  .bar .b-todo {{ background:#9995; color:#fffa; }}
  table {{ border-collapse:collapse; font-size:.85rem; margin-top:.4rem; }}
  th, td {{ text-align:left; padding:.2rem .9rem .2rem 0; }}
  th {{ color:#888; font-weight:500; border-bottom:1px solid #8884; }}
  td.num, th.num {{ text-align:right; font-variant-numeric:tabular-nums; }}
</style>
</head>
<body>
  <h1>{title}</h1>
  <div class="meta">Rooted at #{root} · {total} work item(s) · generated by lazyaz</div>
  {stats_html}
  <div class="cols">
    <div class="col">
      <h2>Hierarchy</h2>
      {hierarchy}
    </div>
    <div class="col">
      <h2>Dependency graph</h2>
      <pre class="mermaid">
flowchart TD
{nodes}{edges}      </pre>
    </div>
  </div>
  <script type="module">
    import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';
    mermaid.initialize({{ startOnLoad: true, securityLevel: 'loose' }});
  </script>
</body>
</html>
"#
    )
}

/// Render the story-points statistics block (summary cards, a done/active/todo
/// burn-down bar, and a per-type breakdown table).
fn render_stats(s: &PointStats) -> String {
    if s.estimated == 0 {
        return format!(
            r#"<div class="stats"><h2>Story points</h2><p class="meta">No story points set on these items ({} unestimated).</p></div>"#,
            s.unestimated
        );
    }
    let pct = |v: f64| if s.total > 0.0 { (v / s.total) * 100.0 } else { 0.0 };
    let seg = |class: &str, label: &str, v: f64| {
        if v <= 0.0 {
            String::new()
        } else {
            format!(
                r#"<span class="{class}" style="width:{:.1}%">{} {}</span>"#,
                pct(v),
                fmt_pts(v),
                label
            )
        }
    };
    let bar = format!(
        r#"<div class="bar">{}{}{}</div>"#,
        seg("b-done", "done", s.done),
        seg("b-active", "active", s.active),
        seg("b-todo", "to do", s.todo),
    );

    let mut rows = String::new();
    for (ty, (pts, count)) in &s.by_type {
        rows.push_str(&format!(
            r#"<tr><td>{}</td><td class="num">{}</td><td class="num">{}</td></tr>"#,
            esc(ty),
            count,
            fmt_pts(*pts),
        ));
    }

    let done_pct = pct(s.done);
    format!(
        r#"<div class="stats">
      <h2>Story points</h2>
      <div class="cards">
        <div class="card"><div class="n">{total}</div><div class="l">total points</div></div>
        <div class="card"><div class="n">{done}</div><div class="l">done ({done_pct:.0}%)</div></div>
        <div class="card"><div class="n">{active}</div><div class="l">in progress</div></div>
        <div class="card"><div class="n">{todo}</div><div class="l">to do</div></div>
        <div class="card"><div class="n">{est}/{all}</div><div class="l">estimated</div></div>
      </div>
      {bar}
      <table>
        <tr><th>Type</th><th class="num">Items</th><th class="num">Points</th></tr>
        {rows}
      </table>
    </div>"#,
        total = fmt_pts(s.total),
        done = fmt_pts(s.done),
        active = fmt_pts(s.active),
        todo = fmt_pts(s.todo),
        est = s.estimated,
        all = s.estimated + s.unestimated,
    )
}

/// Render a node's label spans (id link, type, title, state, assignee).
fn node_label(item: &WorkItem, url_prefix: Option<&str>) -> String {
    let id_html = match url_prefix {
        Some(p) => format!(
            r#"<a class="id" href="{}{}" target="_blank">#{}</a>"#,
            esc(p),
            item.id,
            item.id
        ),
        None => format!(r#"<span class="id">#{}</span>"#, item.id),
    };
    let who = if item.assigned_to.trim().is_empty() {
        String::new()
    } else {
        format!(r#"<span class="who">{}</span>"#, esc(&item.assigned_to))
    };
    let pts = match item.story_points {
        Some(p) if p > 0.0 => format!(r#"<span class="type">{} pts</span>"#, fmt_pts(p)),
        _ => String::new(),
    };
    format!(
        r#"{id_html}<span class="type">{}</span>{}<span class="state {}">{}</span>{pts}{who}"#,
        esc(&item.item_type),
        esc(&item.title),
        state_class(&item.state_name),
        esc(&item.state_name),
    )
}

/// Recursively emit the collapsible hierarchy for a node.
fn render_node(
    id: u32,
    cache: &HashMap<u32, WorkItem>,
    url_prefix: Option<&str>,
    visited: &mut HashSet<u32>,
    out: &mut String,
) {
    if !visited.insert(id) {
        out.push_str(&format!(
            r#"<div class="leaf"><span class="id">#{id}</span> <em>(cycle)</em></div>"#
        ));
        return;
    }
    let Some(item) = cache.get(&id) else {
        out.push_str(&format!(
            r#"<div class="leaf"><span class="id">#{id}</span> <em>(not loaded)</em></div>"#
        ));
        return;
    };
    let label = node_label(item, url_prefix);
    let children: Vec<u32> = item.children.to_vec();
    if children.is_empty() {
        out.push_str(&format!(r#"<div class="leaf">{label}</div>"#));
    } else {
        out.push_str(&format!("<details open><summary>{label}</summary><ul>"));
        for c in children {
            out.push_str("<li>");
            render_node(c, cache, url_prefix, visited, out);
            out.push_str("</li>");
        }
        out.push_str("</ul></details>");
    }
}

/// Walk the subtree collecting Mermaid node definitions and parent→child edges.
fn build_graph(
    id: u32,
    cache: &HashMap<u32, WorkItem>,
    seen: &mut HashSet<u32>,
    nodes: &mut String,
    edges: &mut String,
) {
    if !seen.insert(id) {
        return;
    }
    match cache.get(&id) {
        Some(item) => {
            nodes.push_str(&format!(
                "  n{id}[\"#{id} {}\"]\n",
                mermaid_label(&item.title)
            ));
            for c in &item.children {
                edges.push_str(&format!("  n{id} --> n{c}\n"));
                build_graph(*c, cache, seen, nodes, edges);
            }
        }
        None => {
            nodes.push_str(&format!("  n{id}[\"#{id} (not loaded)\"]\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{WorkItem, WorkItemState};

    fn item(id: u32, title: &str, children: Vec<u32>) -> WorkItem {
        WorkItem {
            id,
            title: title.into(),
            item_type: "User Story".into(),
            state: WorkItemState::Active,
            state_name: "Active".into(),
            available_states: vec![],
            assigned_to: "Ada".into(),
            iteration: String::new(),
            description: String::new(),
            notes: String::new(),
            tags: vec![],
            story_points: None,
            parent: None,
            children,
            dev_links: vec![],
            comments: vec![],
            changed_days_ago: 0,
            rev: 1,
        }
    }

    fn sample() -> HashMap<u32, WorkItem> {
        let mut m = HashMap::new();
        m.insert(1, item(1, "Root epic", vec![2, 3]));
        m.insert(2, item(2, "Child <story> & \"more\"", vec![4]));
        m.insert(3, item(3, "Other child", vec![]));
        m.insert(4, item(4, "Grandchild", vec![]));
        m
    }

    #[test]
    fn renders_all_nodes_and_edges() {
        let html = render_tree_html(1, &sample(), "Tree #1", Some("https://x/edit/"));
        // Every item appears in the graph.
        for id in [1, 2, 3, 4] {
            assert!(html.contains(&format!("n{id}[")), "node {id} missing");
        }
        // Parent → child edges present.
        assert!(html.contains("n1 --> n2"));
        assert!(html.contains("n2 --> n4"));
        // Link prefix applied.
        assert!(html.contains("https://x/edit/1"));
        // Hierarchy uses collapsible details.
        assert!(html.contains("<details open>"));
    }

    #[test]
    fn escapes_html_special_characters() {
        let html = render_tree_html(1, &sample(), "Tree", None);
        // The raw unescaped title must not leak into the document.
        assert!(!html.contains("Child <story>"));
        assert!(html.contains("Child &lt;story&gt;"));
        // Without a url prefix, ids are plain spans (no anchor href).
        assert!(!html.contains("href=\"1\""));
    }

    #[test]
    fn story_point_stats_summarise_subtree() {
        let mut m = HashMap::new();
        let mut epic = item(1, "Epic", vec![2, 3, 4]);
        epic.item_type = "Epic".into();
        m.insert(1, epic);
        let mut done = item(2, "Done story", vec![]);
        done.story_points = Some(5.0);
        done.state_name = "Closed".into();
        m.insert(2, done);
        let mut active = item(3, "Active story", vec![]);
        active.story_points = Some(3.0);
        active.state_name = "Active".into();
        m.insert(3, active);
        let mut todo = item(4, "New story", vec![]);
        todo.story_points = Some(2.0);
        todo.state_name = "New".into();
        m.insert(4, todo);

        let html = render_tree_html(1, &m, "Stats", None);
        assert!(html.contains("Story points"));
        // Total = 10, done = 5 (50%).
        assert!(html.contains(r#"<div class="n">10</div>"#), "total points");
        assert!(html.contains("done (50%)"));
        // Per-type table lists the Epic (0 pts) and User Story (10 pts).
        assert!(html.contains("User Story"));
        // Points badge appears in the hierarchy.
        assert!(html.contains("5 pts"));
    }

    #[test]
    fn story_point_stats_handle_no_estimates() {
        let html = render_tree_html(1, &sample(), "No points", None);
        assert!(html.contains("No story points set"));
    }

    #[test]
    fn handles_missing_and_cyclic_nodes() {
        let mut m = HashMap::new();
        m.insert(1, item(1, "A", vec![2]));
        m.insert(2, item(2, "B", vec![1])); // cycle back to 1
        let html = render_tree_html(1, &m, "Cycle", None);
        assert!(html.contains("(cycle)"));

        let mut m2 = HashMap::new();
        m2.insert(1, item(1, "A", vec![99])); // 99 not in cache
        let html2 = render_tree_html(1, &m2, "Missing", None);
        assert!(html2.contains("(not loaded)"));
    }
}
