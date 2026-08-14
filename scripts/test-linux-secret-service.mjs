#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const SECURE = join(ROOT, 'crates/secure-store');
const errors = [];

// T088: Linux Secret Service adapter + no-service fallback.
const SECURE_TOKENS = [
  'pub enum SecretEnvironment', 'pub fn detect_environment', 'pub enum FallbackPolicy',
  'pub struct LinuxSecretStore', 'pub struct MemoryLinuxStore', 'GnomeKeyring', 'KdeWallet',
  'Headless', 'Refuse', 'MemoryOnly', 'pub fn persisted', 'pub fn environment', 'pub fn fallback',
  'environment_detection_is_explicit', 'headless_refuse_never_touches_disk',
  'headless_memory_only_keeps_secrets_in_memory', 'gnome_and_kde_use_the_secret_service',
];
const FORBIDDEN_DEPENDENCIES = ['vt-parser', 'wezterm-term', 'alacritty_terminal'];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

function checkCrateTokens(crateDir, tokens, label) {
  const files = [];
  if (existsSync(join(crateDir, 'src'))) collectRs(join(crateDir, 'src'), files);
  const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
  for (const token of tokens) {
    if (!sourceText.includes(token)) errors.push(`${label} is missing required token: ${token}`);
  }
}

function checkForbiddenDeps(crateDir, label) {
  const manifest = readFileSync(join(crateDir, 'Cargo.toml'), 'utf8');
  const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
  const depsSection = depsMatch?.[1] ?? '';
  for (const line of depsSection.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
    if (!name) continue;
    if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`${label} has forbidden dependency: ${name}`);
  }
}

if (!existsSync(join(SECURE, 'Cargo.toml'))) errors.push('Missing crates/secure-store/Cargo.toml');
checkCrateTokens(SECURE, SECURE_TOKENS, 'secure-store');
checkForbiddenDeps(SECURE, 'secure-store');

for (const args of [
  ['check', '-p', 'secure-store', '--locked'],
  ['test', '-p', 'secure-store', '--locked'],
]) {
  const crate = args[1];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p ${crate} failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`linux-secret-service contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('linux-secret-service contract valid: LinuxSecretStore targets the D-Bus Secret Service with explicit environment detection (GNOME/KDE/headless); headless + Refuse never stores (no plaintext on disk); headless + MemoryOnly keeps secrets in memory only (persisted() stays false); GNOME/KDE use the service; cargo check/test --locked passed.');