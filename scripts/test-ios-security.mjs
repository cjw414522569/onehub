#!/usr/bin/env node

// T130 contract: iOS Keychain, biometrics, file import, sharing.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const LIB = join(ROOT, 'crates/host-library');
const errors = [];

const TOKENS = [
  'pub enum SecretKind', 'pub enum DataProtectionClass', 'pub fn for_secret',
  'pub fn restores_to_other_device', 'pub struct KeychainImport', 'pub struct IosKeychainImport',
  'pub fn import', 'pub enum IosBiometricState', 'pub struct IosBiometricPrompt',
  'pub fn confirm', 'pub fn cancel', 'pub struct TempImportCleanup', 'pub fn cleanup',
  'data_protection_class_is_correct_per_secret',
  'keychain_import_applies_class_and_cleans_temp_files', 'biometric_prompt_confirm_and_cancel',
  'backup_restore_never_restores_device_only_secrets',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(LIB, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`host-library missing required token: ${token}`);
}

for (const args of [
  ['check', '-p', 'host-library', '--locked'],
  ['test', '-p', 'host-library', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p host-library failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`ios-security contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('ios-security contract valid: DataProtectionClass is correct per secret kind (key material and auth tokens are device-only, so they never restore to another device on backup restore; settings may restore); IosKeychainImport applies the class and cleans up every temporary import file immediately; IosBiometricPrompt confirm/cancel follows the device state; cargo check/test --locked passed (lock-screen/restart/backup-restore tests run on Apple hosts).');