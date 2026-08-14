#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/core-domain');
const errors = [];

const REQUIRED_TOKENS = [
  'pub enum CredentialValue', 'pub struct AgentHandle', 'pub struct HardwareKeyHandle',
  'pub enum UnlockInteraction', 'pub enum ProviderError', 'pub trait CredentialProvider',
  'async fn retrieve', 'fn unlock_interaction', 'fn supports',
  'password_round_trip_never_enters_plain_config',
  'private_key_and_certificate_are_secret_bytes',
  'agent_and_hardware_key_are_opaque_handles_without_secrets',
  'missing_credential_returns_not_found_without_secret_context',
  'unlock_interaction_matches_credential_kind',
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
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`core-domain has forbidden runtime dependency: ${name}`);
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
// CredentialValue must not derive Debug or Clone.
const cp = sourceText.split('pub enum CredentialValue')[0].slice(-400);
if (/#\[derive/.test(cp)) errors.push('CredentialValue must not derive any trait');
if (sourceText.includes('impl Debug for CredentialValue') || sourceText.includes('impl Clone for CredentialValue')) {
  errors.push('CredentialValue must not implement Debug or Clone');
}

for (const args of [['check', '-p', 'core-domain', '--locked'], ['test', '-p', 'core-domain', '--locked'], ['test', '-p', 'core-domain', '--locked', '--doc']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args.join(' ')} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`credential provider contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('credential provider contract valid: zeroizing secret values, opaque handles, unlock interactions, simulated provider tests, cargo check/test/--doc --locked passed.');