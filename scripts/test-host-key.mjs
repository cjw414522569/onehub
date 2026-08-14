#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/core-domain');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum HostKeyPolicy', 'pub enum HostKeyStatus', 'pub enum HostKeyDecision',
  'pub fn verify_host_key', 'pub struct KnownHostsEntry', 'pub fn parse',
  'pub enum KnownHostsMarker', 'pub struct HostKeyIdentity',
  'known_hosts_marker_and_hashed_entries_parse',
  'strict_policy_table_is_explicit',
  'first_trust_policy_prompts_on_unknown_and_rejects_conflicts',
  'certificate_authority_policy_trusts_ca_and_rejects_revoked',
];
const FORBIDDEN_DEPENDENCIES = [
  'russh', 'libssh', 'ssh2', 'sqlx', 'sqlite', 'winui', 'windows-app-sdk',
  'swiftui', 'appkit', 'uikit', 'compose', 'gtk', 'flutter', 'tauri',
  'typescript', 'webview', 'tokio', 'wgpu', 'harfbuzz',
];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/core-domain/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`core-domain has forbidden dependency: ${name}`);
}

const sourceFiles = [];
function collect(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collect(absolute);
    else if (entry.name.endsWith('.rs')) sourceFiles.push(absolute);
  }
}
if (existsSync(join(CRATE, 'src'))) collect(join(CRATE, 'src'));
const sourceText = sourceFiles.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of REQUIRED_TOKENS) {
  if (!sourceText.includes(token)) errors.push(`core-domain is missing required token: ${token}`);
}
for (const marker of ['@cert-authority', '@revoked', '|1|']) {
  if (!sourceText.includes(marker)) errors.push(`known_hosts corpus is missing marker: ${marker}`);
}
for (const status of ['Known', 'Unknown', 'Changed', 'CertificateAuthorized', 'Revoked', 'Mismatch']) {
  if (!sourceText.includes(status)) errors.push(`HostKeyStatus is missing variant: ${status}`);
}

for (const args of [['check', '-p', 'core-domain', '--locked'], ['test', '-p', 'core-domain', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p core-domain failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`host key contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('host key contract valid: policies, statuses, decisions, known_hosts OpenSSH corpus, cargo check/test --locked passed.');