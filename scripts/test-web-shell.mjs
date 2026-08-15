#!/usr/bin/env node

// T139 contract: TypeScript Web/PWA responsive shell, terminal surface,
// and input adapter — type gate, critical-path E2E, and screenshot matrix.

import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const APP = join(ROOT, 'web/app');
const errors = [];

// The committed screenshot matrix (engines x viewports).
const MATRIX = ['chromium-desktop', 'chromium-mobile', 'firefox-desktop', 'safari-mobile'];

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: opts.cwd ?? APP, encoding: 'utf8', timeout: opts.timeout ?? 300000 });
}

// 1. Type gate: strict tsc --noEmit.
const tscBin = join(APP, 'node_modules/typescript/bin/tsc');
if (!existsSync(tscBin)) {
  const install = run('npm.cmd', ['ci'], { timeout: 600000 });
  if (install.status !== 0) errors.push(`npm ci failed:\n${install.stdout}\n${install.stderr}`);
}
const tsc = run('node', [tscBin, '--noEmit', '-p', join(APP, 'tsconfig.json')]);
if (tsc.status !== 0) errors.push(`tsc --noEmit failed:\n${tsc.stdout}\n${tsc.stderr}`);

// 2. Critical-path E2E (headless, deterministic).
const e2e = run('node', ['--experimental-strip-types', join(APP, 'test/e2e.ts')]);
if (e2e.status !== 0) errors.push(`web-shell E2E failed:\n${e2e.stdout}\n${e2e.stderr}`);

// 3. Screenshot matrix: regenerate and compare byte-for-byte with committed.
const temp = mkdtempSync(join(tmpdir(), 'web-shell-matrix-'));
const gen = run('node', ['--experimental-strip-types', join(APP, 'test/screenshot-matrix.ts'), '--write', temp], {
  cwd: APP,
});
if (gen.status !== 0) {
  errors.push(`screenshot matrix generation failed:\n${gen.stdout}\n${gen.stderr}`);
} else {
  for (const name of MATRIX) {
    const committed = join(APP, 'screenshots', `${name}.svg`);
    const generated = join(temp, `${name}.svg`);
    if (!existsSync(committed)) {
      errors.push(`committed screenshot missing (regenerate with --write): ${name}.svg`);
      continue;
    }
    if (!existsSync(generated)) {
      errors.push(`generated screenshot missing: ${name}.svg`);
      continue;
    }
    if (!readFileSync(committed).equals(readFileSync(generated))) {
      errors.push(`screenshot ${name}.svg is not byte-identical to the committed golden`);
    }
  }
}
rmSync(temp, { recursive: true, force: true });

if (errors.length > 0) {
  console.error(`web-shell contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('web-shell contract valid: strict TypeScript compiles; critical paths (responsive layout, gateway handshake, terminal render, input adapter, resize, mobile switch, disconnect) pass headlessly; the Chromium/Firefox/Safari desktop+mobile screenshot matrix regenerates byte-identical.');