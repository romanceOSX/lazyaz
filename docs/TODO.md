# TODO

Actionable follow-ups for the real Azure DevOps backend. Context and rationale
live in `future-improvements.md`; this is the short checklist.

- [ ] **Rebuild backend after the wizard.** The real `AzureClient` is built once
      at startup from `config.org_url`/`project` (`main::build_backends`). Running
      `--setup`/the first-run wizard while signed in updates the config but not the
      live client, so org/project changes need a restart. Let the wizard
      reconstruct the client + authenticator in place.

- [x] **OAuth refresh-token rotation.** The `offline_access` refresh token is now
      persisted in `token.json` and used to mint fresh access tokens silently:
      `OAuthAuthenticator::ensure_token` reuses a valid token, else renews via the
      (rotated) refresh token, else falls back to the interactive device-code
      flow. So the user signs in interactively only ~every few months (refresh-
      token lifetime) instead of ~hourly. Still at *startup* only — mid-session
      access-token expiry isn't renewed yet (the client header is fixed at
      launch); see the note below.

- [ ] **Keychain token storage.** Token cache is plaintext at
      `config_dir/token.json` (0600). Move to the OS keychain (a lighter
      `keyring` feature set than the one removed earlier).

- [x] **"Notes" pane bound to the ADO field named "Notes".** Its reference name
      is resolved at runtime from the project's fields metadata (it's usually a
      custom field like `Custom.Notes`, not `System.Notes`), cached per process,
      with `System.Notes` as a last-resort fallback. Not user-configurable.

- [x] **Push timeframe filter into WIQL.** The real backend now builds the
      `[System.ChangedDate]` clause (and iteration `UNDER` clause) server-side
      from `WorkItemFilter`; the mock filters in-memory via `filter.matches()`.

- [ ] **End-to-end verification against a real org.** The 7.1 request/response
      shapes are coded to the docs but untested live (no account/PAT available
      during implementation).

- [ ] **Per-comment conflict detection.** Comment edits are last-write-wins;
      the rev/conflict flow is field-scoped only.

- [ ] **Navigation approach** Update the navigation approach;
      Review claude's skill store navigation ('/' for filtering or searching in the current pane, vim motions for navigation)

## latest todo's

- [x] Add a way to delete comments
- [x] The item's notes do not match what the azure web UI shows
- [x] Add a 'tags' field that shows the item's current tabs, we should be able to fuzzy find across these tags and add additional ones
- [x] Is there a way to query the available item's status?, the current ones don't match what is in the azure web UI
- [x] Add a development section indicating the related links such as github links and such
- [x] When we edit a window and the border gets yellow-colored, the 'current-selection' color gets shadowed, make this color selection to 'overlay' the underlying color
      not just overide it with the current color

## filter by.. timeframes and iterations

Iterations have the following characteristics
- A time-window (begin date and end date)
- They might vary depending on the begin and end dates

A timeframe (purely an lazyaz's concept) has the following characteristics
- Begin date
- End date

Ideally we would like to filter by the following:
- Filter by Iterations (select one or multiple iterations to show, most of the time we would only select one (the current one))
- Filter by timeframe (what is the work assigned between date A and date B)

What would be an ergonomic way of achieving this within the TUI?

### Implemented

- [x] **Filter by iterations.** `i` (in the Tree / Work Items panes) opens a
      floating fuzzy multi-select of the team's iterations; Space/Enter toggles,
      Tab applies, Esc cancels. Defaults to the current sprint on first load.
      Selected paths persist across sessions and are pushed into WIQL as
      `[System.IterationPath] UNDER '…'` clauses (mock matches by prefix).
- [x] **Filter by timeframe.** `f`/`F` cycle the quick presets (Today / This
      week / This month / All) and `c` opens a custom `from…to` date-range
      entry (`Timeframe::Custom`). The custom menu accepts typed `YYYY-MM-DD`
      dates *or* `c` spawns a month calendar (←↑↓→ / `hjkl` move, `[`/`]` change
      month, Enter sets start then end, ordered automatically).
- [x] **Iteration- vs timeframe-based filtering are mutually exclusive.** Only
      one time filter is active at a time: choosing iterations resets the
      timeframe to the neutral `All`, and selecting any timeframe window (preset
      or custom) clears the iteration selection. The status bar shows a single
      time chip reflecting whichever is active.
- [x] **Filter by item type.** `t` (Work Items pane) opens a floating fuzzy
      multi-select of the work-item types (Task / User Story / Feature /
      Capability / Epic); Space/Enter toggles, Tab applies, Esc cancels.
      Defaults to User Stories + Features on first load. Selected types persist
      across sessions and are pushed into WIQL as a `[System.WorkItemType] IN
      (…)` clause (mock matches in-memory).
- [x] **Active-filter chips** in the status bar show the current iteration,
      timeframe, and item type (`iter:… · tf:… · type:…`).

## Work Items
- [x] **Terminology.** The ADO umbrella concept is a **Work Item** (a User Story,
      Task, Bug, Epic, Feature, etc. are all *types* of work item). So the correct
      term is "Work Item(s)", not "Working Items" — which the code already uses
      (`WorkItem`, `WorkItemClient`, the "Work Items" tab). Doc heading fixed.
- [x] **`v` → view item in tree.** In the Work Items pane, `v` switches to the
      Tree tab centred on the selected item.
- [x] **Tree is a relationship view, independent of the filters.** The Tree tab
      no longer mirrors the (timeframe/iteration/type) filtered list. It has its
      own dataset (`App::tree_items`) — the connected relationship graph
      (ancestors + descendants) of a focus item (`App::tree_focus`), fetched
      directly via `client.get` so the timeframe filter doesn't hide related
      items. The focus is the selected item on `v`, or the open/selected item
      when you switch to the Tree tab. Fetch is synchronous on that navigation;
      a background fetch (like the list refresh worker) is a possible follow-up.
- [x] **Work Items is the default landing window.** `Tab::default()` and the
      startup tab are now `WorkItems`; combined with the iteration filter
      defaulting to the current sprint on first load, you land on the current
      iteration's work items.

## Re-designing the app
- [x] Status: implemented

Summary of what changed:
- `Timeframe` is now a `{ from: Option<Date>, to: Option<Date> }` window (was a
  preset enum). Open-ended ranges work: start-only → `[System.ChangedDate] >=`,
  end-only → `<=`, both unset → no constraint.
- The `f`/`F` preset cycling is gone. `f` (Work Items) now opens the timeframe
  window (`OpenTimeframeFilter`); `i` opens the iteration picker. The two are
  still mutually exclusive (one resets the other).
- New timeframe window (`src/ui/date_range.rs`): Start/End rows, each toggled
  on/off with Space (open-ended). `h`/`l` (←/→) move between Y/M/D fields; `k`/↑
  increase, `j`/↓ decrease, or type digits. `c` opens the calendar
  (`src/ui/calendar.rs`) to pick the range visually.
- Tree view: already independent of the time filters, already not the boot
  window (Work Items is), and `v` already opens it centred on the current item
  with parents, children and siblings uncollapsed.

### The concept of 'time filters'
*Note that this only applies to the 'Work Items' window, but it may impact some other windows
int the future*

Time filters are entities that filter the displayed *Work Items* for the initial stage there should
be two kinds of *Time filters*:
- Time frame filter
- Iteration Filters
**Only one type of filter can exist at the time**

### Iteration filters
These filters filter by Iteration, the user will select them by the use of the Itereation
fzf picker (which I believe it is already in place, implemented)

### Time frame filter
This type of filter represent a 'time window', the user should be able to select an
starting time and end time on a new floating window
When selecting the date the user should be able to increment the value under the cursor
if the user uses 'j'/down arrow to decrease, 'k'/up arrow to increase, or type it manually
If the user uses 'l'/right arrow, or 'h' left arrow, they will be able to navigate to the
next field of the date.

Additionally the user will also be able to select the 'range' date visually from
a 'calendar'

If the user only declares an start date, then the filter will query the work items above
that date, on the other hand if the user just selects an end date, the query will show all the
stories up to that day

### Things to remove
Right now if we press 'f' on the Work Items window we switch around the 'time filter',
remove this, this is not necesarry anymore.

### moving the 'Tree view' window
The Tree view is independent of the time filters (for now), it should not be the initial
window when the app boots, it should be the 'Work Items' one.
Additionally the user is able to spawn the Tree View with the cursor under the current work item when
they press the key 'v', they will get redirected to the 'Tree View' window with the current work item with its
children uncollapsed, their parents, and their siblings uncollapsed, the user will be able to uncollapse and navigate
through the tree

### Considerations
Literally, feel free to remove any unwanted code or do any massive refactoring, we do not care about breaking
jhanges or maintaing any code, this is a Proof of Concept

## Tree View Window
- [x] Status: implemented

Implementation notes:
- **Tab order** is now `[Work Items, Detail, Tree, Config]` (`Tab::ORDER`): Tree
  sits after Work Items (and Detail) and before Config.
- **Lazy, level-by-level walking.** The tree no longer fetches the whole
  connected component. Anchored on a focus item (`v`), it shows the parent (one
  up), the focus + its siblings (current level), and the focus's children (one
  down). Deeper levels load only when you expand a node (`l`), which fetches just
  that node's direct children. A collapsed node with children shows `▸`
  (continues down); a `⋯` row at the top means there are more ancestors above.
- **Cache + refresh.** Fetched items live in `App::tree_cache` and are reused as
  you walk. `r` (in the Tree pane) re-fetches the current tree from scratch.
- Files: `App::{tree_cache,tree_focus,tree_root}`, `TreeRow`,
  `load_tree_for`/`tree_expand_node`/`tree_flatten` in `src/app.rs`;
  `src/ui/tree.rs` rendering.

Original notes:

The tree view should not be the first int the tabs, it should be placed before the 'config' tab, after the 'Work Items' tab
Conceptually the Tree View is a 'tree' diagram whose main purpose is showing the relationships among work items in a more intuitive and
graphical way to the user, since ther are tons of work items in an organization we don't want to show everything, so we will
rely in the user 'walking' the tree to progressively query the related parents, children, and siblings.

### Interaction

When the user opens the 'tree view' when hitting 'v' on a work item, it should get navigated
to the tree view, under the current work item, the children of such work item should be displayed, at least up to the next
'nest' level, it should not query all its children yet
Ideally the user will only get displayed the current 'level', one before, and one after, with '...' indicating that the tree
continues in such direction.

The application should 'cache' these relationships, if the user wants to 'refresh' the tree, they should be able to do so through a
keybinding. (note that this behaviour might change in the future)

## Considerations
Literally, feel free to remove any unwanted code or do any massive refactoring, we do not care about breaking
jhanges or maintaing any code, this is a Proof of Concept

# Doubts
- [x] Is there a way to get notified whenever a Work Item is added under a certain Work Item? Or is polling the only pawsible sollution?

  **Answer: there are real push options, but none of them is a client-side
  "subscribe" call — so for a local TUI like lazyaz, polling is the practical
  path.** Breakdown:

  - **Service Hooks (webhooks)** — Azure DevOps' first-class push mechanism. You
    create a subscription for the `workitem.created` (or `workitem.updated`)
    event, optionally filtered by area path / work-item type, and ADO POSTs a
    JSON payload to an HTTPS endpoint you control. This is true push, no polling.
    The catch for us: it needs a **publicly reachable server** to receive the
    POST — a terminal app has nowhere to deliver it. You'd need a small relay
    service (and there's no "notify me when a child is added under item X"
    filter specifically; you'd subscribe to created events and check the parent
    link yourself).
  - **No client subscribe / long-poll in the REST API.** There is no supported
    endpoint a client can hold open to be told "an item changed". The web UI
    uses an internal SignalR feed, but that's not a public/supported API.
  - **Efficient polling** is therefore the answer for lazyaz. Two good options:
    - WIQL/`$expand` by `[System.ChangedDate] >= @lastChecked` to find recently
      changed items, or query an item's children directly and diff.
    - The **Reporting Work Item Revisions** endpoint
      (`_apis/wit/reporting/workItemRevisions`) returns a `continuationToken`
      "watermark", designed for incremental polling — you re-poll from the last
      watermark and only get what changed. This is the cheapest way to poll
      broadly.
  - **Practical recommendation:** poll a parent's children (or a changed-since
    watermark) on the existing background-refresh cadence; offer a manual
    refresh key (already done for the tree: `r`). If real-time push ever becomes
    a hard requirement, add a companion webhook relay — out of scope for a TUI
    PoC.

## Fuzzy finder
- [] Status: implemented

- [ ] We should be able to 'fuzzy filer' the items in any window, though as for an initial approach this
   should only apply to the 'Work-Items' and 'Tree-View' windows, by hitting '/' we should be able to 
   spawn in a 'fzf-style filter' and query any item that contains our request in the
   current window

