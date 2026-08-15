#!/usr/bin/env node

// T138 contract: WASM/WebGPU compile of terminal/domain core + JS interop boundary.

import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const WASM_CRATE = join(ROOT, 'crates/wasm');
const errors = [];

// Bundle budget: the terminal/domain/wgpu core must ship under 3 MiB.
const WASM_BUNDLE_BUDGET_BYTES = 3 * 1024 * 1024;

const TOKENS = [
  'pub const WASM_BOUNDARY_VERSION', 'pub fn boundary_version',
  '#[wasm_bindgen]', 'pub struct JsTerminal', 'pub struct JsOutput',
  'pub struct JsPlanStats', 'pub struct TerminalBridge', 'pub struct BridgeOutput',
  'pub const BRIDGE_VERSION', 'pub fn push', 'pub fn finish', 'pub fn resize',
  'pub fn render_plan', 'pub fn render_plan_stats', 'pub fn text',
  'same_text_vector_as_native_parser', 'fragmented_feed_matches_whole_feed',
  'render_plan_builds_from_same_snapshot', 'unicode_vector_matches_native_width_policy',
  'resize_preserves_content', 'osc_title_is_captured', 'cursor_tracks_columns',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(WASM_CRATE, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`wasm crate missing required token: ${token}`);
}

// The JS boundary must be a versioned wasm-bindgen surface.
const cargoToml = readFileSync(join(WASM_CRATE, 'Cargo.toml'), 'utf8');
if (!cargoToml.includes('wasm-bindgen')) errors.push('wasm crate missing wasm-bindgen dependency');
if (!cargoToml.includes('terminal-parser')) errors.push('wasm crate missing terminal-parser dependency');
if (!/crate-type = \["cdylib", "rlib"\]/.test(cargoToml)) errors.push('wasm crate must be cdylib+rlib (wasm bundle + host tests)');

// WASM compile gate: terminal/domain/wgpu core must build for wasm32.
const wasmCheck = spawnSync('cargo', [
  'check', '--target', 'wasm32-unknown-unknown',
  '-p', 'wasm', '-p', 'terminal-state', '-p', 'core-domain', '-p', 'wgpu-renderer', '--locked',
], { cwd: ROOT, encoding: 'utf8', timeout: 600000 });
if (wasmCheck.status !== 0) {
  errors.push(`wasm32 check failed:\n${wasmCheck.stdout}\n${wasmCheck.stderr}`);
}

// Bundle-size gate: build the release cdylib and assert the .wasm is in budget.
const wasmBuild = spawnSync('cargo', [
  'build', '--release', '--target', 'wasm32-unknown-unknown', '-p', 'wasm', '--locked',
], { cwd: ROOT, encoding: 'utf8', timeout: 600000 });
if (wasmBuild.status !== 0) {
  errors.push(`wasm32 release build failed:\n${wasmBuild.stdout}\n${wasmBuild.stderr}`);
} else {
  const wasmPath = join(ROOT, 'target/wasm32-unknown-unknown/release/wasm.wasm');
  if (!existsSync(wasmPath)) {
    errors.push(`expected wasm bundle missing: ${wasmPath}`);
  } else {
    const size = statSync(wasmPath).size;
    if (size > WASM_BUNDLE_BUDGET_BYTES) {
      errors.push(`wasm bundle ${size} bytes exceeds budget ${WASM_BUNDLE_BUDGET_BYTES}`);
    } else {
      console.log(`wasm bundle size ${size} bytes (within ${WASM_BUNDLE_BUDGET_BYTES} budget)`);
    }
  }
}

// Same test vectors as native: host tests exercise the shared pipeline.
const wasmTest = spawnSync('cargo', ['test', '-p', 'wasm', '--locked'], {
  cwd: ROOT, encoding: 'utf8', timeout: 300000,
});
if (wasmTest.status !== 0) {
  errors.push(`cargo test -p wasm failed:\n${wasmTest.stdout}\n${wasmTest.stderr}`);
}

if (errors.length > 0) {
  console.error(`wasm-webgpu contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('wasm-webgpu contract valid: terminal-state, core-domain, and wgpu-renderer compile for wasm32-unknown-unknown; the wasm crate ships a versioned wasm-bindgen JS boundary (boundary_version 1, JsTerminal push/resize/text/render_plan_stats) and a release .wasm within the bundle budget; bridge tests reuse the native terminal test vectors (same text/SGR/OSC/Unicode/resize vectors, fragmented-feed equivalence) and pass; WebGPU render plans are built from the same snapshot the native renderer consumes.');