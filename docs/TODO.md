# TODO

Actionable follow-ups for the real Azure DevOps backend. Context and rationale
live in `future-improvements.md`; this is the short checklist.

- [ ] **Rebuild backend after the wizard.** The real `AzureClient` is built once
      at startup from `config.org_url`/`project` (`main::build_backends`). Running
      `--setup`/the first-run wizard while signed in updates the config but not the
      live client, so org/project changes need a restart. Let the wizard
      reconstruct the client + authenticator in place.

- [ ] **OAuth refresh-token rotation.** We request `offline_access` but don't use
      the refresh token; on access-token expiry the user must `--login` again.
      Persist + rotate the refresh token in `src/auth/oauth.rs`.

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
      week / This sprint / All) and `c` opens a custom `from…to` date-range
      entry (`Timeframe::Custom`). Both compose (AND) with the iteration filter.
- [x] **Filter by item type.** `t` (Work Items pane) opens a floating fuzzy
      multi-select of the work-item types (Task / User Story / Feature /
      Capability / Epic); Space/Enter toggles, Tab applies, Esc cancels.
      Defaults to User Stories + Features on first load. Selected types persist
      across sessions and are pushed into WIQL as a `[System.WorkItemType] IN
      (…)` clause (mock matches in-memory).
- [x] **Active-filter chips** in the status bar show the current iteration,
      timeframe, and item type (`iter:… · tf:… · type:…`).

## Working Items
- [ ] Is 'Working items' the correct term? what is the term for referring to an item in
      ADO? a concept that encapsulates User Stories, Tasks, Epics, Features and so on
- [ ] When hitting 'v' we should get redirected to a 'tree' view of the current item being navigated
      this tree will show us its parents and children


