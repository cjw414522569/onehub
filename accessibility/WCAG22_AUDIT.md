# Accessibility / i18n / multi-input-device final audit (T162)

Status: **PASS** on August 15, 2026 (verified_at_utc=2026-08-15T06:50:00Z).

## WCAG 2.2 AA

`accessibility/wcag22-audit.json` - 12 success criteria on the critical
paths: 10 pass, 2 partial (with remediation plans), 0 fail. Automated
contrast over the design-system tokens: all 5 declared text pairs meet
4.5:1 and the focus ring meets 3:1 non-text contrast.

## i18n

`accessibility/i18n-audit.json` - `messages.en.json` and `messages.zh-CN.json`
share identical message IDs (7), plurals/dates/RTL/truncation checks pass via
`lint-i18n.mjs`. A native-widget hardcoded-string scan remains a platform
gate.

## Multi-input devices

`accessibility/input-matrix.json` - a 6x6 matrix (keyboard / mouse / touch /
stylus / IME / gamepad x navigate / select / connect / type / scroll /
cancel) with every cell documented; keyboard/mouse/touch/IME pass, stylus
partial, gamepad blocked (platform gates).

## Verification

```text
node scripts/test-a11y-audit.mjs .  PASS
automated contrast 5/5 >= AA (focus ring 3:1)  PASS
12 WCAG criteria: 10 pass / 2 partial w/ remediation / 0 fail  PASS
i18n catalogs share 7 IDs; lint-i18n pass  PASS
input matrix 6x6 all documented  PASS
manual device matrix: blocked_unavailable_toolchain  documented
powershell -File .\scripts\test.ps1 -SkipPlatform  PASS
powershell -File .\scripts\lint.ps1 -SkipPlatform  PASS
node .\scripts\validate-workspace.mjs .  PASS
powershell -File .\scripts\validate-architecture-rules.ps1  PASS
powershell -File .\scripts\validate-control.ps1  PASS
```