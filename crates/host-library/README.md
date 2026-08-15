# host-library

- Layer: L1 (core services)
- Dependencies: `core-domain`
- Scope: host library model (list / group / tags / search / sort).

## T102: host library list, group, tags, search, and sort

`crates/host-library/src/host.rs`:

- `HostRecord` - id, name, host, port, group, sorted tags, last-used.
- `HostLibrary` - add/insert/remove/get, case-insensitive `search` across
  name/host/tags/group, `filter_by_tag`, `groups` / `tags` summaries, and
  deterministic `sorted` (name / host / group / last-used, asc/desc).
- `SelectionModel` - stable index-based navigation (next/prev/first/last with
  clamping) so keyboard and touch operate the list identically.
- `view` - a deterministic text view (golden-testable).
- 10k-host performance test: search / filter / sort stay well under an
  interactive budget, and selection navigates the full list.

## T103: host editor with inline validation

`crates/host-library/src/editor.rs`:

- `HostEditorForm` - five reviewable sections (basic / auth / proxy /
  terminal / advanced), each with labeled fields in focus order.
- Inline validation: every field revalidates on `set` (including cross-field
  rules such as auth-method `key` requiring a key path, and proxy-enabled
  requiring host/port); the form is valid only when all fields pass.
- `review()` - an auditable view that masks secrets (`PASSWORD_MASK`) so the
  configuration can be reviewed before saving.
- `accessibility()` - every field is labeled, focus order is stable, and
  every error message is non-empty (screen-reader friendly).

## T104: first-connection host fingerprint review

`crates/host-library/src/fingerprint.rs`:

- `HostKeyFingerprint` - deterministic SHA-256 fingerprint of raw key bytes
  (sha2), colon-grouped display, key algorithm, and source.
- `FingerprintReview` - classifies a presented fingerprint against a known
  one: new host (medium risk), matching known (algorithm risk), changed key
  (high risk with a change notice). Approve / reject drive the review state.
- `ReviewView` - the UI shows algorithm, SHA-256 fingerprint, source, risk,
  and an explicit change notice for a key change.