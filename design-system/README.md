# Design System (T100)

Cross-platform design tokens, theme baselines, and responsive breakpoints.

## Source of truth

`design-system/tokens.json`:

- `primitives` - raw color, typography, spacing, radius, and breakpoint
  values (the only place concrete values live).
- `semantic` - role-based tokens that reference primitives (e.g.
  `color.text.primary -> gray.900`, `color.accent -> blue.600`). Native
  implementations consume the semantic layer, so every platform shares the
  same meaning and visual baseline.
- `high_contrast` - overrides of the semantic layer (and extension tokens such
  as `focus.ring`). The theme is extensible: adding a role here resolves and
  is covered by the contrast rules.
- `contrast_pairs` + `min_contrast_ratio` - WCAG contrast rules enforced for
  the baseline and high-contrast themes.

## Validation

- `scripts/lint-design-tokens.mjs` - token lint: schema, resolvable
  references, strictly increasing breakpoints, and WCAG contrast for every
  declared pair in both themes.
- `scripts/test-design-system.mjs` - lint + reproducible golden snapshots:
  resolving the baseline and high-contrast themes must produce byte-identical
  files (`design-system/golden/*.snapshot.json`) - CI detects drift.

## Platform status (Windows-first)

- Shared semantic baseline + high-contrast overrides: implemented and golden-
  tested on this host.
- Per-platform rendering goldens (Windows UI, SwiftUI, Compose, etc.) run on
  their native hosts and are `blocked_environment` here; they consume the
  same `semantic` tokens.