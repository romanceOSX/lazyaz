# Future improvements

Deferred ideas, captured so we don't lose them.

## Multiple open buffers (multi-item workspace)

Today the app holds a **single** open work item (`App.current`) and session
restore persists just that one item. A richer model would let you keep several
items "open" at once and switch between them like editor buffers / browser tabs.

Sketch of what it would take:

- Replace `current: Option<WorkItem>` with `open: Vec<WorkItem>` + an `active:
  usize` index (or a small `Buffers` struct).
- A buffer bar / picker to switch between open items (could reuse
  `src/ui/picker.rs`).
- Keys: open-in-new-buffer, next/prev buffer, close buffer.
- Session persistence (`src/session.rs`): persist `Vec<u32>` of open item ids +
  the active index instead of a single `current_id`. This part is small — the
  bulk of the work is the in-app buffer model and navigation UX.
- Conflict handling already keys off the item being edited, so it generalizes
  per-buffer with little change.

Decision (2026-06-09): ship single-item restore first; revisit buffers later.

## Real backend — known limitations

The real Azure DevOps backend is wired (`src/api/azure.rs`, `src/auth/oauth.rs`,
`src/auth/pat.rs`; backend chosen in `main::build_backends`). Outstanding gaps:

- **Backend is fixed at startup.** The real `AzureClient` is constructed from
  `config.org_url`/`project` before the TUI starts. If you run the first-run
  wizard while signed in (incomplete config), the wizard updates the config but
  the live client isn't rebuilt — restart to pick up the new org/project. A
  proper fix lets the wizard reconstruct the client/authenticator in-place.
- **Token cache is plaintext** at `config_dir/token.json` (0600). Move to the OS
  keychain.
- **OAuth token isn't refreshed** mid-session; on expiry you must `--login`
  again. We request `offline_access`, so wiring refresh-token rotation is a
  small follow-up.
- **"Notes" maps to `Microsoft.VSTS.Common.AcceptanceCriteria`** — there's no
  universal Notes field. Make this configurable per process template.
- **Timeframe filtering is client-side** over `changed_days_ago` derived from
  `System.ChangedDate`; could push into the WIQL `WHERE` instead.
- The `X` "simulate teammate edit" key only works on the mock backend.

## Other candidates

- Per-comment conflict detection (currently comment edits are last-write-wins).
- Keychain token storage (see above).
