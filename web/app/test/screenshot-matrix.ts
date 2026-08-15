// Screenshot matrix generator (T139): renders deterministic SVG frames of
// the terminal surface for the Chromium / Firefox / Safari desktop and
// mobile matrix. Run with --write to (re)generate web/app/screenshots/*.svg.

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createShell } from '../src/index.ts';
import { Key } from '../src/input-adapter.ts';
import type { Viewport } from '../src/shell.ts';

/** The output directory: an explicit argument wins, else screenshots/. */
function resolveOutDir(): string {
  const flagIndex = process.argv.indexOf('--write');
  const positional = process.argv
    .slice(flagIndex >= 0 ? flagIndex + 1 : 1)
    .find((arg) => !arg.startsWith('-'));
  if (positional) return resolve(positional);
  return join(dirname(fileURLToPath(import.meta.url)), '..', 'screenshots');
}

const OUT_DIR = resolveOutDir();

interface MatrixEntry {
  name: string;
  engine: string;
  viewport: Viewport;
  label: string;
}

const MATRIX: MatrixEntry[] = [
  { name: 'chromium-desktop', engine: 'chromium', viewport: { width: 1280, height: 720 }, label: 'Chromium Desktop' },
  { name: 'chromium-mobile', engine: 'chromium', viewport: { width: 390, height: 844 }, label: 'Chromium Mobile' },
  { name: 'firefox-desktop', engine: 'firefox', viewport: { width: 1280, height: 720 }, label: 'Firefox Desktop' },
  { name: 'safari-mobile', engine: 'safari', viewport: { width: 390, height: 844 }, label: 'Safari Mobile' },
];

async function runSession(viewport: Viewport): Promise<ReturnType<typeof createShell>> {
  const shell = createShell(viewport);
  shell.selectHost('dev.example.com');
  await shell.connect('short-lived-token-matrix');
  for (const key of ['l', 's', ' ', '-', 'l', 'a']) {
    shell.sendKey({ key: Key.Char, char: key, modifiers: { shift: false, alt: false, ctrl: false } });
  }
  shell.sendKey({ key: Key.Enter, modifiers: { shift: false, alt: false, ctrl: false } });
  return shell;
}

async function main(): Promise<void> {
  const write = process.argv.includes('--write');
  const entries: { name: string; svg: string }[] = [];
  for (const entry of MATRIX) {
    const shell = await runSession(entry.viewport);
    entries.push({ name: entry.name, svg: shell.terminal().toSvg(entry.engine, entry.label) });
  }

  if (write) {
    mkdirSync(OUT_DIR, { recursive: true });
    for (const { name, svg } of entries) {
      const file = join(OUT_DIR, `${name}.svg`);
      writeFileSync(file, svg, 'utf8');
      console.log(`wrote ${file} (${svg.length} bytes)`);
    }
  } else {
    for (const entry of MATRIX) {
      console.log(`matrix entry: ${entry.name} (${entry.engine}, ${entry.viewport.width}x${entry.viewport.height})`);
    }
  }
}

void main();