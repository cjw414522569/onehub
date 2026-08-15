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