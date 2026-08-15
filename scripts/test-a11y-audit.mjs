#!/usr/bin/env node

// T162 contract: accessibility / i18n / multi-input-device final audit.
// Runs automated WCAG 2.2 AA checks (contrast math over the design-system
// tokens), validates the WCAG conformance matrix, the i18n catalogs, and the
// input-device matrix. The manual device matrix is documented
// blocked_unavailable_toolchain. With --write, archives the report.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];
const AA_CONTRAST = 4.5;
const NON_TEXT_CONTRAST = 3.0;

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: opts.timeout ?? 600000 });
}
function luminance(hex) {
  const rgb = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
    .map((c) => (c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4)));
  return 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
}
function contrast(a, b) {
  const l1 = luminance(a); const l2 = luminance(b);
  const hi = Math.max(l1, l2); const lo = Math.min(l1, l2);
  return (hi + 0.05) / (lo + 0.05);
}

// 1. Automated WCAG contrast: design-system semantic pairs must meet AA.
const tokens = JSON.parse(readFileSync(join(ROOT, 'design-system/tokens.json'), 'utf8'));
const prim = tokens.primitives.color;
const semantic = tokens.semantic.color;
function resolveColor(name) {
  const ref = semantic[name] ?? name;
  const hex = prim[ref] ?? ref;
  if (!/^#[0-9a-fA-F]{6}$/.test(hex)) throw new Error(`cannot resolve color: ${name}`);
  return hex;
}
const contrastResults = [];
for (const [fg, bg] of tokens.contrast_pairs) {
  const ratio = contrast(resolveColor(fg), resolveColor(bg));
  contrastResults.push({ fg, bg, ratio: Number(ratio.toFixed(2)), pass: ratio >= AA_CONTRAST });
  if (ratio < AA_CONTRAST) errors.push(`WCAG contrast fail: ${fg} on ${bg} = ${ratio.toFixed(2)} < ${AA_CONTRAST}`);
}
// Focus ring (non-text contrast) must meet 3:1.
const focusRatio = contrast(resolveColor('focus'), resolveColor('surface'));
contrastResults.push({ pair: 'focus/surface', ratio: Number(focusRatio.toFixed(2)), pass: focusRatio >= NON_TEXT_CONTRAST });
if (focusRatio < NON_TEXT_CONTRAST) errors.push(`focus ring contrast ${focusRatio.toFixed(2)} < ${NON_TEXT_CONTRAST}`);

// 2. WCAG 2.2 AA conformance matrix.
const wcag = JSON.parse(readFileSync(join(ROOT, 'accessibility/wcag22-audit.json'), 'utf8'));
if (wcag.schema_version !== 1) errors.push('wcag audit schema_version != 1');
for (const criterion of wcag.criteria ?? []) {
  if (criterion.status === 'fail') errors.push(`WCAG ${criterion.id} failed`);
  if (!['pass', 'partial'].includes(criterion.status)) errors.push(`WCAG ${criterion.id} has invalid status`);
  if (criterion.status === 'partial' && !criterion.remediation) errors.push(`WCAG ${criterion.id} partial without remediation`);
}

// 3. i18n final audit: en/zh-CN catalogs share identical message IDs.
const en = JSON.parse(readFileSync(join(ROOT, 'i18n/messages.en.json'), 'utf8'));
const zh = JSON.parse(readFileSync(join(ROOT, 'i18n/messages.zh-CN.json'), 'utf8'));
const enKeys = Object.keys(en).sort();
const zhKeys = Object.keys(zh).sort();
if (JSON.stringify(enKeys) !== JSON.stringify(zhKeys)) errors.push('i18n catalogs do not share identical message IDs');
const i18nLint = run('node', [join(ROOT, 'scripts/lint-i18n.mjs'), ROOT]);
if (i18nLint.status !== 0) errors.push(`i18n lint failed:\n${i18nLint.stdout}\n${i18nLint.stderr}`);

// 4. Multi-input device matrix: every cell has a documented status.
const matrix = JSON.parse(readFileSync(join(ROOT, 'accessibility/input-matrix.json'), 'utf8'));
const valid = new Set(['pass', 'partial', 'n/a', 'blocked']);
for (const device of matrix.devices) {
  for (const action of matrix.actions) {
    const status = matrix.cells[device]?.[action];
    if (!valid.has(status)) errors.push(`input matrix ${device}/${action} has invalid status: ${status}`);
  }
}

if (errors.length > 0) {
  console.error(`a11y-audit contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

// 5. Archive the report.
const report = {
  task: 'T162',
  status: 'pass',
  verified_at_utc: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
  wcag: {
    standard: wcag.standard,
    criteria: wcag.criteria.map((c) => ({ id: c.id, status: c.status })),
    automated_contrast: contrastResults,
  },
  i18n: { shared_message_ids: enKeys.length, catalogs: ['messages.en.json', 'messages.zh-CN.json'] },
  input_matrix: { devices: matrix.devices.length, actions: matrix.actions.length, documented_cells: matrix.devices.length * matrix.actions.length },
  manual_device_matrix: 'blocked_unavailable_toolchain',
};
const reportPath = join(ROOT, 'docs/reports/A11Y_AUDIT_T162.json');
if (process.argv.includes('--write')) {
  mkdirSync(dirname(reportPath), { recursive: true });
  writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  console.log(`wrote ${reportPath}`);
}

console.log(`a11y-audit contract valid: ${wcag.criteria.length} WCAG 2.2 AA criteria (${wcag.criteria.filter((c) => c.status === 'pass').length} pass, ${wcag.criteria.filter((c) => c.status === 'partial').length} partial w/ remediation, 0 fail); automated contrast ${contrastResults.filter((r) => r.pass).length}/${contrastResults.length} pairs >= AA; i18n catalogs share ${enKeys.length} IDs; input matrix ${matrix.devices.length}x${matrix.actions.length} all documented; manual device matrix blocked_unavailable_toolchain.`);