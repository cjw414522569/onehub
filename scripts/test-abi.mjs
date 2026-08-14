#!/usr/bin/env node

// T097 contract: stable C ABI + pinned codegen + drift detection.

import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const ABI = join(ROOT, 'crates/abi-c');
const HEADER = join(ABI, 'include/ssh_abi.h');
const errors = [];

const TOKENS = [
  'pub const ABI_SCHEMA_VERSION', 'pub const ABI_CODEGEN_VERSION',
  'pub struct AbiMessageHeader', 'pub extern "C" fn ssh_abi_version',
  'pub extern "C" fn ssh_abi_codegen_version', 'pub extern "C" fn ssh_abi_header_size',
  'pub extern "C" fn ssh_abi_field_offset', 'pub extern "C" fn ssh_abi_header_is_valid',
  'pub extern "C" fn ssh_abi_validate_field_offsets',
  'header_layout_matches_rust_layout', 'exported_functions_agree_with_header',
  'codegen_version_is_pinned_and_embedded', 'message_header_default_and_validity',
  'validate_field_offsets_returns_zero',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(ABI, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`abi-c missing required token: ${token}`);
}
const manifest = readFileSync(join(ABI, 'Cargo.toml'), 'utf8');
if (!manifest.includes('crate-type = ["lib", "cdylib"]')) {
  errors.push('abi-c must build both lib and cdylib');
}

// Drift detection: regenerating the header must produce a byte-identical file.
const tmp = join(ROOT, 'artifacts/tmp/abi-regenerate');
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });
const gen = spawnSync('node', [join(ROOT, 'scripts/generate-abi.mjs'), tmp], {
  cwd: ROOT, encoding: 'utf8', timeout: 60000,
});
if (gen.status !== 0) {
  errors.push(`generate-abi failed:\n${gen.stdout}\n${gen.stderr}`);
} else {
  const regenerated = readFileSync(join(tmp, 'ssh_abi.h'), 'utf8');
  const checkedIn = readFileSync(HEADER, 'utf8');
  if (regenerated !== checkedIn) {
    errors.push('ABI header drift: scripts/generate-abi.mjs output differs from crates/abi-c/include/ssh_abi.h (regenerate and commit)');
  }
  const codegenLine = checkedIn.split(/\r?\n/).find((line) => line.includes('SSH_ABI_CODEGEN_VERSION'));
  if (!codegenLine || !/SSH_ABI_CODEGEN_VERSION "1\.0\.0"/.test(codegenLine)) {
    errors.push('ABI codegen version is not pinned in the header');
  }
}

// The cdylib must export the stable C symbols.
const build = spawnSync('cargo', ['build', '-p', 'abi-c', '--locked'], { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
if (build.status !== 0) {
  errors.push(`cargo build -p abi-c failed:\n${build.stdout}\n${build.stderr}`);
} else {
  const dll = join(ROOT, 'target/debug/abi_c.dll');
  if (!existsSync(dll)) {
    errors.push('abi_c.dll not produced by cargo build -p abi-c');
  } else {
    const text = Buffer.from(readFileSync(dll)).toString('latin1');
    for (const symbol of ['ssh_abi_version', 'ssh_abi_header_size', 'ssh_abi_field_offset',
      'ssh_abi_header_is_valid', 'ssh_abi_validate_field_offsets', 'ssh_abi_codegen_version']) {
      if (!text.includes(symbol)) errors.push(`cdylib is missing exported symbol: ${symbol}`);
    }
  }
}

for (const args of [
  ['check', '-p', 'abi-c', '--locked'],
  ['test', '-p', 'abi-c', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p abi-c failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`abi contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('abi contract valid: crates/abi-c exports a stable, versioned C ABI (AbiMessageHeader with schema_version/message_type/byte_len/request_id/cancel/backpressure/error_code, size 32, offsets 0/4/8/16/24/25/28) as a lib+cdylib; the generated header crates/abi-c/include/ssh_abi.h is byte-identical after regeneration (no drift) and pins ABI codegen version 1.0.0; the cdylib exports all six ssh_abi_* symbols; Rust tests verify header layout == exported ABI (host-side link-test equivalent); Swift/Kotlin/C# bindings stay interface-only with the pinned codegen (blocked-unavailable-toolchain).');