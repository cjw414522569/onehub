#!/usr/bin/env node

import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { validateSupplyChain } from './validate-supply-chain.mjs';

const ROOT = resolve(process.argv[2] ?? process.cwd());

function write(file, content) {
  const parent = file.slice(0, Math.max(file.lastIndexOf('/'), file.lastIndexOf('\\')));
  if (parent) mkdirSync(parent, { recursive: true });
  writeFileSync(file, content, 'utf8');
}

function buildFixture() {
  const root = mkdtempSync(join(tmpdir(), 'ssh-supply-chain-'));
  write(join(root, 'Cargo.lock'), '# empty lockfile fixture\n');
  write(join(root, 'toolchains/toolchain.lock.json'), '{"schema_version":1}\n');
  write(join(root, 'supply-chain/lockfiles.json'), JSON.stringify({
    schema_version: 1,
    lockfiles: [
      { path: 'Cargo.lock', kind: 'cargo', purpose: 'fixture lockfile' },
      { path: 'toolchains/toolchain.lock.json', kind: 'toolchain', purpose: 'fixture toolchain lock' },
    ],
  }, null, 2));
  write(join(root, 'supply-chain/vulnerability-exceptions.json'), JSON.stringify({ schema_version: 1, exceptions: [] }, null, 2));
  write(join(root, 'supply-chain/secret-exceptions.json'), JSON.stringify({ schema_version: 1, exceptions: [] }, null, 2));
  write(join(root, 'artifacts/reports/SBOM_CDX.json'), JSON.stringify({
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    version: 1,
    metadata: { timestamp: '2026-08-14T00:00:00.000Z', component: { type: 'application', name: 'fixture', version: '0.1.0' } },
    components: [{ type: 'library', 'bom-ref': 'pkg:cargo/fixture@1.0.0', name: 'fixture', version: '1.0.0' }],
    dependencies: [],
  }, null, 2));
  write(join(root, 'artifacts/reports/LICENSE_COMPLIANCE.json'), JSON.stringify({ status: 'pass', release_blockers: [] }, null, 2));
  write(join(root, 'artifacts/reports/VULNERABILITY_SCAN.json'), JSON.stringify({
    schema_version: 1, tool: 'cargo-audit', tool_available: false, status: 'blocked_environment', findings: [],
  }, null, 2));
  write(join(root, 'artifacts/reports/SECRET_SCAN.json'), JSON.stringify({
    schema_version: 1, tool: 'ssh-client-scan-secrets', status: 'pass', files_scanned: 0, findings: [],
  }, null, 2));
  return root;
}

function expectPass(root, label) {
  const { errors } = validateSupplyChain(root);
  assert.deepEqual(errors, [], `${label} should pass; errors=${JSON.stringify(errors)}`);
}

function expectErrors(root, label, predicate, count) {
  const { errors } = validateSupplyChain(root);
  assert.ok(Array.isArray(errors) && errors.length > 0, `${label} should fail`);
  if (typeof predicate === 'function') {
    assert.ok(errors.some((error) => predicate(error)), `${label} errors should match predicate; errors=${JSON.stringify(errors)}`);
  }
  if (typeof count === 'number') assert.equal(errors.length, count, `${label} error count mismatch; errors=${JSON.stringify(errors)}`);
  return errors;
}

// Positive: the real repository gate must pass.
const positive = validateSupplyChain(ROOT);
assert.deepEqual(positive.errors, [], `real repo should pass supply chain gate; errors=${JSON.stringify(positive.errors)}`);

// Negative fixtures.
{
  const root = buildFixture();
  try {
    expectPass(root, 'baseline fixture');
    write(join(root, 'supply-chain/lockfiles.json'), JSON.stringify({
      schema_version: 1,
      lockfiles: [{ path: 'Cargo.missing.lock', kind: 'cargo', purpose: 'missing fixture' }],
    }, null, 2));
    expectErrors(root, 'missing lockfile', (e) => /lockfile is missing/.test(e));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
{
  const root = buildFixture();
  try {
    const expired = new Date(Date.now() - 86400000).toISOString().slice(0, 10);
    write(join(root, 'supply-chain/vulnerability-exceptions.json'), JSON.stringify({
      schema_version: 1,
      exceptions: [{ match: 'RUSTSEC-0000-0000', owner: 'team', reason: 'fixture', expires_at: expired }],
    }, null, 2));
    expectErrors(root, 'expired vulnerability exception', (e) => /expired/.test(e));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
{
  const root = buildFixture();
  try {
    write(join(root, 'artifacts/reports/VULNERABILITY_SCAN.json'), JSON.stringify({
      schema_version: 1, tool: 'cargo-audit', tool_available: true, status: 'available',
      findings: [{ id: 'RUSTSEC-9999-9999', package: 'example', version: '1.0.0', severity: 'high', title: 'fixture high' }],
    }, null, 2));
    expectErrors(root, 'high vulnerability without exception', (e) => /blocking vulnerability without exception/.test(e));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
{
  const root = buildFixture();
  try {
    write(join(root, 'artifacts/reports/VULNERABILITY_SCAN.json'), JSON.stringify({
      schema_version: 1, tool: 'cargo-audit', tool_available: true, status: 'available',
      findings: [{ id: 'RUSTSEC-9999-9999', package: 'example', version: '1.0.0', severity: 'high', title: 'fixture high' }],
    }, null, 2));
    const future = new Date(Date.now() + 86400000).toISOString().slice(0, 10);
    write(join(root, 'supply-chain/vulnerability-exceptions.json'), JSON.stringify({
      schema_version: 1,
      exceptions: [{ match: 'RUSTSEC-9999-9999', owner: 'team', reason: 'fixture', expires_at: future }],
    }, null, 2));
    expectPass(root, 'high vulnerability with valid exception');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
{
  const root = buildFixture();
  try {
    write(join(root, 'artifacts/reports/SECRET_SCAN.json'), JSON.stringify({
      schema_version: 1, tool: 'ssh-client-scan-secrets', status: 'findings',
      findings: [{ path: 'src/leak.txt', line: 1, rule: 'aws-access-key-id' }],
    }, null, 2));
    expectErrors(root, 'secret finding without exception', (e) => /secret finding without exception/.test(e));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
{
  const root = buildFixture();
  try {
    rmSync(join(root, 'artifacts/reports/SBOM_CDX.json'), { force: true });
    expectErrors(root, 'missing SBOM', (e) => /SBOM_CDX\.json/.test(e));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
{
  const root = buildFixture();
  try {
    write(join(root, 'artifacts/reports/LICENSE_COMPLIANCE.json'), JSON.stringify({ status: 'pass', release_blockers: [{ id: 'fixture-blocker' }] }, null, 2));
    expectErrors(root, 'license release blockers', (e) => /empty release_blockers array/.test(e));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

console.log('Supply chain gate contract tests passed: baseline, missing lockfile, expired exception, blocking vulnerability, valid exception, secret finding, missing SBOM, license release blockers.');