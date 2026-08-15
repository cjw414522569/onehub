#!/usr/bin/env node

import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join, relative, resolve, sep } from 'node:path';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const OUT = resolve(ROOT, 'artifacts/reports/SECRET_SCAN.json');
const SKIP_DIRS = new Set(['.git', 'target', 'node_modules', '.claude', '.codex', '.idea', '.vscode', 'testdata', 'artifacts']);
const SKIP_FILES = new Set(['Cargo.lock']);
const MAX_SCAN_BYTES = 4 * 1024 * 1024;

const RULES = [
  // A private key is only a real finding when a full PEM block is present
  // (BEGIN header + base64 body + END footer). A bare `-----BEGIN ... PRIVATE
  // KEY-----` marker inside parser/test source is not a secret.
  { rule: 'private-key', pattern: /-----BEGIN [A-Z0-9 ]*PRIVATE KEY( BLOCK)?-----\s*[A-Za-z0-9+/=\s]{40,}?-----END [A-Z0-9 ]*PRIVATE KEY-----/ },
  { rule: 'aws-access-key-id', pattern: /\bAKIA[0-9A-Z]{16}\b/ },
  { rule: 'aws-secret-access-key', pattern: /\baws_secret_access_key\s*[=:]\s*[A-Za-z0-9/+=]{40}\b/i },
  { rule: 'github-pat', pattern: /\b(?:ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22,})\b/ },
  { rule: 'slack-token', pattern: /\bxox[baprs]-[A-Za-z0-9-]{10,}\b/ },
  { rule: 'stripe-live-key', pattern: /\bsk_live_[0-9a-zA-Z]{24,}\b/ },
  { rule: 'google-api-key', pattern: /\bAIza[0-9A-Za-z_-]{35}\b/ },
  { rule: 'npm-token', pattern: /\bnpm_[A-Za-z0-9]{36}\b/ },
];

function isBinary(buffer) {
  const probe = buffer.subarray(0, 8192);
  return probe.includes(0);
}

function walk(dir, findings, state) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue;
      walk(absolute, findings, state);
      continue;
    }
    if (!entry.isFile()) continue;
    if (SKIP_FILES.has(entry.name)) continue;
    let stat;
    try {
      stat = statSync(absolute);
    } catch {
      continue;
    }
    if (stat.size <= 0 || stat.size > MAX_SCAN_BYTES) continue;
    let buffer;
    try {
      buffer = readFileSync(absolute);
    } catch {
      continue;
    }
    if (isBinary(buffer)) continue;
    const text = buffer.toString('utf8');
    const rel = relative(ROOT, absolute).split(sep).join('/');
    state.filesScanned += 1;
    // The private-key rule matches a full PEM block across the whole file
    // (one finding per file); the token rules stay per-line.
    const privateKeyRule = RULES.find((r) => r.rule === 'private-key');
    if (privateKeyRule.pattern.test(text)) {
      findings.push({ path: rel, line: 1, rule: 'private-key' });
    }
    const lines = text.split(/\r?\n/);
    for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
      const line = lines[lineIndex];
      for (const { rule, pattern } of RULES) {
        if (rule === 'private-key') continue;
        if (pattern.test(line)) {
          findings.push({ path: rel, line: lineIndex + 1, rule });
        }
      }
    }
  }
}

const findings = [];
const state = { filesScanned: 0 };
walk(ROOT, findings, state);

const report = {
  schema_version: 1,
  tool: 'ssh-client-scan-secrets',
  generated_at_utc: new Date().toISOString(),
  status: findings.length > 0 ? 'findings' : 'pass',
  files_scanned: state.filesScanned,
  patterns_checked: RULES.map((r) => r.rule),
  findings,
};

mkdirSync(resolve(ROOT, 'artifacts/reports'), { recursive: true });
writeFileSync(OUT, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
console.log(`secret scan: ${report.status} (files_scanned=${state.filesScanned}, findings=${findings.length}, report=${relative(ROOT, OUT).split(sep).join('/')})`);
if (report.status === 'findings') process.exitCode = 0;