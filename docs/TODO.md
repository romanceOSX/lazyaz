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

- [ ] **Configurable "Notes" field.** Currently hard-mapped to
      `Microsoft.VSTS.Common.AcceptanceCriteria` (`NOTES_FIELD` in
      `src/api/azure.rs`); there's no universal Notes field. Make it configurable
      per process template.

- [ ] **Push timeframe filter into WIQL.** Filtering is client-side over
      `changed_days_ago`; add a `[System.ChangedDate] >= @Today - N` clause.

- [ ] **End-to-end verification against a real org.** The 7.1 request/response
      shapes are coded to the docs but untested live (no account/PAT available
      during implementation).

- [ ] **Per-comment conflict detection.** Comment edits are last-write-wins;
      the rev/conflict flow is field-scoped only.
