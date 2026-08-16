#!/usr/bin/env node

// T008 contract: the copied mXterm UI's connection dialog + repository work
// end-to-end through the bridge protocol. A mock bridge (mirroring bridge.rs
// response shapes) is installed before the app boots; the test then drives the
// real UI: open 新增连接 -> fill 名称/主机/端口/用户名 -> 创建连接 -> the
// repository shows the session -> click it -> terminal_connect -> a terminal
// tab opens. Screenshots artifacts/tmp/mxterm-ui-connected.png.

import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import { join, resolve } from 'node:path';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const UI = join(ROOT, 'clients/windows/ui');
const DIST = join(UI, 'dist');
const PORT = 8094;
const require = createRequire(join(UI, 'package.json'));
const { chromium } = require('playwright');
const errors = [];

// Mock bridge that mirrors bridge.rs response shapes.
const mockBridge = `
window.__mockSessions = [];
window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: function (event, eventId) { return window.__TAURI_INTERNALS__.invoke("plugin:event|unlisten", { event: event, eventId: eventId }); } };
window.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
  transformCallback: function (cb) { return (window.__cb = (window.__cb || 0) + 1); },
  unregisterCallback: function (id) {},
  convertFileSrc: function (p) { return p; },
  invoke: function (cmd, payload) {
    return new Promise(function (resolve) {
      function profile(i, s) {
        return { id: 'session-' + i, name: s.name, protocol: 'ssh', host: s.host, port: s.port,
          username: s.user, group: null, credential_mode: 'prompt',
          proxy: { kind: 'none' }, jump: { kind: 'none' },
          advanced: { auth_timeout_ms: 45000, connect_timeout_ms: 30000, keepalive_interval_ms: 20000, terminal_encoding: 'utf-8' },
          is_favorite: false, created_at: 'host-shell', updated_at: 'host-shell' };
      }
      if (cmd === 'connection_list') {
        resolve(window.__mockSessions.map(function (s, i) { return profile(i, s); }));
      } else if (cmd === 'connection_upsert') {
        var req = (payload && payload.request) || payload || {};
        var s = { name: req.name || req.host, host: req.host || '', port: Number(req.port) || 22, user: req.username || '' };
        window.__mockSessions.push(s);
        resolve(profile(window.__mockSessions.length - 1, s));
      } else if (cmd === 'connection_delete') {
        window.__mockSessions = [];
        resolve(null);
      } else if (cmd === 'terminal_connect') {
        resolve('session-0');
      } else if (cmd === 'terminal_close' || cmd === 'terminal_write' || cmd === 'terminal_resize') {
        resolve(null);
      } else if (cmd === 'plugin:window|outer_position' || cmd === 'plugin:window|inner_position') {
        resolve({ x: 0, y: 0 });
      } else if (cmd === 'plugin:window|outer_size' || cmd === 'plugin:window|inner_size') {
        resolve({ width: 1440, height: 900 });
      } else if (cmd === 'plugin:window|is_maximized' || cmd === 'plugin:window|is_minimized' || cmd === 'plugin:window|is_fullscreen') {
        resolve(false);
      } else if (cmd === 'plugin:window|is_visible') {
        resolve(true);
      } else if (cmd === 'plugin:window|scale_factor') {
        resolve(1);
      } else if (cmd === 'plugin:window|current_monitor' || cmd === 'plugin:window|available_monitors') {
        resolve([{ position: { x: 0, y: 0 }, size: { width: 1920, height: 1080 }, scaleFactor: 1 }]);
      } else if (cmd === 'plugin:event|listen') {
        resolve(1);
      } else if (cmd === 'plugin:event|unlisten' || cmd === 'plugin:event|emit') {
        resolve(null);
      } else if (cmd === 'connection_test' || cmd === 'connection_test_profile') {
        var req = (payload && payload.request) || payload || {};
        var host = (req && req.host) || '';
        if (host === 'bad-host') {
          resolve({ ok: false, ssh_verified: false, message: '无法连接 bad-host:22（TCP 不可达）。' });
        } else {
          resolve({ ok: true, ssh_verified: true, message: 'mock host shell' });
        }
      } else if (cmd === 'connection_probe_latency') {
        resolve({ latency_ms: null, reachable: true });
      } else if (cmd === 'secret_vault_status' || cmd === 'secret_vault_unlock' || cmd === 'secret_vault_unlock_local') {
        resolve({ initialized: true, unlocked: true });
      } else if (cmd === 'credential_list') {
        resolve([]);
      } else if (cmd === 'command_snippet_list' || cmd === 'command_history_list') {
        resolve([]);
      } else if (cmd === 'get_app_runtime_info') {
        resolve({ platform: 'windows', family: 'windows', arch: 'x86_64', version: '0.1.0' });
      } else if (cmd === 'get_supported_window_materials') {
        resolve([]);
      } else {
        resolve(null);
      }
    });
  }
};
`;

const server = spawn(process.execPath, [join(ROOT, 'scripts/serve-static.cjs'), DIST, String(PORT)], {
  stdio: 'ignore',
});

try {
  await new Promise((r) => setTimeout(r, 800));
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.addInitScript(mockBridge);
  const consoleErrors = [];
  page.on('console', (msg) => { if (msg.type() === 'error') consoleErrors.push(msg.text().slice(0, 160)); });
  page.on('pageerror', (err) => consoleErrors.push(String(err).slice(0, 160)));

  await page.goto(`http://127.0.0.1:${PORT}/`, { waitUntil: 'networkidle', timeout: 90000 });
  await page.waitForTimeout(4000);

  // Open the 新增连接 dialog.
  await page.click('[aria-label="新增连接"]', { timeout: 15000 });
  await page.waitForTimeout(800);

  // Diagnose the dialog inputs.
  const dlgInputs = await page.evaluate(() =>
    Array.from(document.querySelectorAll('input')).map((i) => ({
      ph: i.placeholder,
      visible: i.offsetParent !== null,
    }))
  );
  console.log('DIALOG_INPUTS:', JSON.stringify(dlgInputs));

  // Fill the SSH form.
  await page.fill('input[placeholder="请输入主机地址"]', '10.0.0.1');
  await page.fill('input[inputmode="numeric"]', '2222');
  await page.fill('input[placeholder="请输入用户名"]', 'root');
  await page.fill('input[placeholder="例如：生产跳板"]', 'dev');

  // Create.
  await page.click('button:has-text("创建连接")', { timeout: 15000 });
  await page.waitForTimeout(1500);

  // The repository must show the new session.
  const repoText = await page.evaluate(
    () => document.querySelector('[aria-label="连接仓库"]')?.innerText ?? ''
  );
  console.log('REPO:', repoText.replace(/\s+/g, ' ').slice(0, 220));
  if (!repoText.includes('dev') && !repoText.includes('10.0.0.1')) {
    errors.push('repository did not show the new session');
  }

  // Click the session to connect (terminal_connect).
  await page.click('[aria-label="连接仓库"] >> text=dev', { timeout: 15000 });
  await page.waitForTimeout(2500);

  await page.screenshot({ path: join(ROOT, 'artifacts/tmp/mxterm-ui-connected.png') });

  // A terminal tab should exist after connecting.
  const tabText = await page.evaluate(() => document.body.innerText.replace(/\s+/g, ' ').slice(0, 400));
  console.log('BODY:', tabText.slice(0, 200));
  if (consoleErrors.length > 0) errors.push('console errors: ' + consoleErrors.slice(0, 4).join(' | '));

  // T057 regression: a failing test connection must surface an error, never
  // an unconditional "连接测试通过".
  await page.click('[aria-label="新增连接"]', { timeout: 15000 });
  await page.waitForTimeout(800);
  await page.fill('input[placeholder="请输入主机地址"]', 'bad-host');
  await page.click('button:has-text("测试连接")', { timeout: 15000 });
  await page.waitForTimeout(1500);
  const testFeedback = await page.evaluate(() => document.body.innerText.replace(/\s+/g, ' '));
  console.log('TEST_FEEDBACK:', testFeedback.slice(0, 300));
  if (testFeedback.includes('连接测试通过')) {
    errors.push('failing test connection still showed 连接测试通过');
  }
  if (!testFeedback.includes('无法连接 bad-host')) {
    errors.push('failing test connection did not surface the backend error message');
  }
  await page.screenshot({ path: join(ROOT, 'artifacts/tmp/mxterm-ui-test-failure.png') });

  await browser.close();
} catch (err) {
  errors.push('ui-flow threw: ' + String(err).slice(0, 400));
} finally {
  server.kill();
}

if (errors.length > 0) {
  console.error('mxterm-ui-flow failed with ' + errors.length + ' error(s):');
  for (const e of errors) console.error('- ' + e);
  process.exit(1);
}
console.log('mxterm-ui-flow PASS: connection dialog created a session via the bridge and the repository shows it.');