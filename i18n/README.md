# i18n (T118)

Native-client internationalization infrastructure with Chinese/English
initial copy.

## Resources

- `i18n/messages.en.json` - English copy (shared message IDs).
- `i18n/messages.zh-CN.json` - Simplified Chinese copy (identical message IDs,
  matching placeholders).
- `i18n/snapshots/*.snapshot.json` - reproducible per-locale snapshots.

## Contract

- Shared message IDs: both locales have identical key sets (enforced).
- Placeholders (`{name}`) match between locales for every message.
- Plurals: `*.one` / `*.other` pairs exist in every locale (zh-CN declares
  the `other` category only, per Chinese plural rules).
- Dates: `format.short`, `format.long`, `time.hms` per locale.
- RTL: `rtl.supported`, mirrored layouts, and the RTL locale list.
- Truncation: every message is within `max_message_chars`, and the
  pseudo-localized (expanded) form stays within 1.5x - so no string risks
  truncation after translation.

## Validation

- `scripts/lint-i18n.mjs` - resource lint.
- `scripts/test-i18n.mjs` - lint + pseudo-localization + per-locale
  snapshots (regenerate -> no diff).