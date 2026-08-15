#!/usr/bin/env node

// T004 contract: the mxterm UI frontend is buildable and renders without a
// Tauri runtime. Verifies:
//   1) dist artifacts (index.html + hashed /assets bundle) exist after
//      `cd clients/windows/ui && npm.cmd run build`
//   2) the bridge shim (src/bridge/shim.ts) defines window.__TAURI_INTERNALS__
//      and forwards to the host bridge (chrome.webview.postMessage)
//   3) the built app renders in Playwright chromium (title "mXterm",
//      non-trivial body text, zero console errors, screenshot saved).
//
// Dev-only prerequisites (not committed): in clients/windows/ui run
//   npm.cmd install --no-save playwright && npx.cmd playwright install chromium

import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const UI = join(ROOT, 'clients/windows/ui');
const DIST = join(UI, 'dist');
const PORT = 8093;
const errors = [];

// 1) dist artifacts.
const indexPath = join(DIST, 'index.html');
if (!existsSync(indexPath)) {
  errors.push('dist/index.html missing (run: cd clients/windows/ui && npm.cmd run build)');
} else {
  const html = readFileSync(indexPath, 'utf8');
  const assetMatch = html.match(/src="\/assets\/([^"]+\.js)"/);
  if (!assetMatch) {
    errors.push('dist/index.html must reference a hashed /assets/*.js bundle');
  } else if (!existsSync(join(DIST, 'assets', assetMatch[1]))) {
    errors.push(`dist/assets/${assetMatch[1]} missing`);
  }
}
const assetCount = existsSync(join(DIST, 'assets'))
  ? readdirSync(join(DIST, 'assets')).filter((f) => f.endsWith('.js')).length
  : 0;
if (assetCount < 10) errors.push(`dist/assets has only ${assetCount} js chunks (expected the code-split app)`);

// 2) bridge shim.
const shimPath = join(UI, 'src/bridge/shim.ts');
if (!existsSync(shimPath)) {
  errors.push('ui/src/bridge/shim.ts missing');
} else {
  const shim = readFileSync(shimPath, 'utf8');
  if (!shim.includes('__TAURI_INTERNALS__')) errors.push('shim must define window.__TAURI_INTERNALS__');
  if (!shim.includes('chrome?.webview')) errors.push('shim must forward to the host bridge via chrome.webview.postMessage');
}

// 3) render check (Playwright chromium, policy-free headless browser).
const require = createRequire(join(UI, 'package.json'));
let chromium = null;
try {
  chromium = require('playwright').chromium;
} catch {
  errors.push('playwright not installed in ui/ (npm.cmd install --no-save playwright && npx.cmd playwright install chromium)');
}

if (chromium && errors.length === 0) {
  const server = spawn(process.execPath, [join(ROOT, 'scripts/serve-static.cjs'), DIST, String(PORT)], {
    stdio: 'ignore',
  });
  try {
    await new Promise((resolve) => setTimeout(resolve, 800));
    const browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    const consoleErrors = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') consoleErrors.push(msg.text().slice(0, 200));
    });
    page.on('pageerror', (err) => consoleErrors.push(String(err).slice(0, 200)));
    await page.goto(`http://127.0.0.1:${PORT}/`, { waitUntil: 'networkidle', timeout: 90000 });
    await page.waitForTimeout(5000);
    const title = await page.title();
    const bodyLen = await page.evaluate(() => document.body.innerText.replace(/\s+/g, ' ').trim().length);
    await page.screenshot({ path: join(ROOT, 'artifacts/tmp/mxterm-render-contract.png') });
    await browser.close();
    if (title !== 'mXterm') errors.push(`render title=${title} (expected mXterm)`);
    if (bodyLen < 500) errors.push(`render body too short (${bodyLen} chars)`);
    if (consoleErrors.length > 0) errors.push(`render console errors: ${consoleErrors.slice(0, 5).join(' | ')}`);
  } catch (err) {
    errors.push(`render check threw: ${String(err).slice(0, 300)}`);
  } finally {
    server.kill();
  }
}

if (errors.length > 0) {
  console.error(`mxterm-build contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(
  `mxterm-build contract valid: dist has ${assetCount} js chunks + entry html; shim defines __TAURI_INTERNALS__; built app renders without Tauri (title=mXterm, body>500 chars, 0 console errors).`
);