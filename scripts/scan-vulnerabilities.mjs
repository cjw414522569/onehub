#!/usr/bin/env node

import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const OUT = resolve(ROOT, 'artifacts/reports/VULNERABILITY_SCAN.json');

const BLOCKING_SEVERITIES = new Set(['high', 'critical']);

function normalizeFindings(auditJson) {
  const findings = [];
  const list = auditJson?.vulnerabilities?.list ?? [];
  for (const entry of list) {
    const advisory = entry?.advisory ?? {};
    const pkg = entry?.package ?? {};
    findings.push({
      id: advisory.id ?? 'unknown',
      package: pkg.name ?? 'unknown',
      version: pkg.version ?? 'unknown',
      severity: String(advisory.severity ?? 'unknown').toLowerCase(),
      title: advisory.title ?? '',
      aliases: Array.isArray(advisory.aliases) ? advisory.aliases : [],
    });
  }
  return findings;
}

mkdirSync(resolve(ROOT, 'artifacts/reports'), { recursive: true });

const probe = spawnSync('cargo', ['audit', '--version'], { encoding: 'utf8' });
let report;
if (probe.error || probe.status !== 0) {
  report = {
    schema_version: 1,
    tool: 'cargo-audit',
    tool_available: false,
    status: 'blocked_environment',
    generated_at_utc: new Date().toISOString(),
    reason: 'cargo-audit is not installed in this environment; the advisory scan was not run locally.',
    findings: [],
  };
} else {
  const scan = spawnSync('cargo', ['audit', '--json'], { cwd: ROOT, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (scan.status !== 0) {
    report = {
      schema_version: 1,
      tool: 'cargo-audit',
      tool_available: true,
      status: 'error',
      generated_at_utc: new Date().toISOString(),
      reason: `cargo audit exited ${scan.status}: ${String(scan.stderr ?? '').slice(0, 2000)}`,
      findings: [],
    };
  } else {
    let parsed;
    try {
      parsed = JSON.parse(scan.stdout);
    } catch (error) {
      report = {
        schema_version: 1,
        tool: 'cargo-audit',
        tool_available: true,
        status: 'error',
        generated_at_utc: new Date().toISOString(),
        reason: `cargo audit produced unparseable output: ${error.message}`,
        findings: [],
      };
    }
    if (!report) {
      const findings = normalizeFindings(parsed);
      report = {
        schema_version: 1,
        tool: 'cargo-audit',
        tool_available: true,
        status: 'available',
        generated_at_utc: new Date().toISOString(),
        blocking_severities: [...BLOCKING_SEVERITIES],
        findings,
      };
    }
  }
}

writeFileSync(OUT, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
const blocking = report.findings?.filter((f) => BLOCKING_SEVERITIES.has(f.severity)) ?? [];
console.log(`vulnerability scan: status=${report.status} tool_available=${report.tool_available} findings=${report.findings?.length ?? 0} blocking_high_critical=${blocking.length} report=artifacts/reports/VULNERABILITY_SCAN.json`);