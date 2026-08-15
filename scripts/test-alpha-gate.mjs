#!/usr/bin/env node

// T170 contract: Alpha entry gate. Runs the core-area contracts (connect,
// terminal, keys, log security) plus the full gates, and requires every one
// to pass. With --write, archives release/alpha/alpha-gate.report.json.

import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const results = [];
const errors = [];

function runContract(name, cmd, args) {
  const res = spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', timeout: 1200000 });
  results.push({ area: name.split('.')[0], contract: name, pass: res.status === 0 });
  if (res.status !== 0) errors.push(`${name} failed:\n${res.stdout?.slice(0, 2000)}\n${res.stderr?.slice(0, 2000)}`);
}

// Core connect.
for (const name of ['test-gateway-session-protocol', 'test-gateway-auth', 'test-gateway-address-policy', 'test-cli-contract', 'test-cli-capabilities', 'test-gateway-soak']) {
  runContract(name, 'node', [join(ROOT, `scripts/${name}.mjs`), ROOT]);
}
// Terminal.
for (const name of ['test-terminal-parser', 'test-input-protocol', 'test-wasm-webgpu']) {
  runContract(name, 'node', [join(ROOT, `scripts/${name}.mjs`), ROOT]);
}
// Keys.
for (const name of ['test-secure-store', 'test-field-encryption']) {
  runContract(name, 'node', [join(ROOT, `scripts/${name}.mjs`), ROOT]);
}
// Log security.
for (const name of ['test-telemetry-logging', 'test-telemetry-privacy', 'test-crash-diagnostics', 'test-diagnostics']) {
  runContract(name, 'node', [join(ROOT, `scripts/${name}.mjs`), ROOT]);
}
// Full workspace gates.
const full = spawnSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', join(ROOT, 'scripts/test.ps1'), '-SkipPlatform'], { cwd: ROOT, encoding: 'utf8', timeout: 1800000 });
results.push({ area: 'workspace', contract: 'test.ps1 -SkipPlatform', pass: full.status === 0 });
if (full.status !== 0) errors.push('test.ps1 failed');

if (errors.length > 0) {
  console.error(`alpha-gate failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

if (process.argv.includes('--write')) {
  const report = {
    task: 'T170', status: 'pass',
    verified_at_utc: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    areas: {
      core_connect: results.filter((r) => r.area === 'core_connect' || ['test-gateway-session-protocol', 'test-gateway-auth', 'test-gateway-address-policy', 'test-cli-contract', 'test-cli-capabilities', 'test-gateway-soak'].includes(r.contract)).map((r) => ({ contract: r.contract, pass: r.pass })),
      terminal: results.filter((r) => ['test-terminal-parser', 'test-input-protocol', 'test-wasm-webgpu'].includes(r.contract)).map((r) => ({ contract: r.contract, pass: r.pass })),
      keys: results.filter((r) => ['test-secure-store', 'test-field-encryption'].includes(r.contract)).map((r) => ({ contract: r.contract, pass: r.pass })),
      log_security: results.filter((r) => ['test-telemetry-logging', 'test-telemetry-privacy', 'test-crash-diagnostics', 'test-diagnostics'].includes(r.contract)).map((r) => ({ contract: r.contract, pass: r.pass })),
    },
    workspace: 'pass',
    total_contracts: results.length,
  };
  const reportPath = join(ROOT, 'release/alpha/alpha-gate.report.json');
  mkdirSync(dirname(reportPath), { recursive: true });
  writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  console.log(`wrote ${reportPath}`);
}

console.log(`alpha-gate contract valid: ${results.length} contracts passed across core connect (gateway session/auth/address/CLI/soak), terminal (parser/input/wasm), keys (secure-store/field-encryption), and log security (logging/privacy/crash/diagnostics); full test.ps1 green. Alpha is usable internally.`);