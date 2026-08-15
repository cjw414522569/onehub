// PWA offline / update / cache-clear critical-path E2E (T140):
// service-worker upgrade + purge, cache policy + audit, and offline
// connectivity (offline never claims connectivity, caches hold no session
// secrets). With --write, (re)generates pwa/cache-audit.snapshot.json.

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { CacheEntry } from '../src/cache-policy.ts';
import { CachePolicy } from '../src/cache-policy.ts';
import { OfflinePolicy } from '../src/connectivity.ts';
import { ServiceWorkerModel } from '../src/service-worker.ts';
import { createShell } from '../src/index.ts';

let failures = 0;

function check(name: string, condition: boolean, detail = ''): void {
  if (condition) {
    console.log(`PASS ${name}`);
  } else {
    failures += 1;
    console.error(`FAIL ${name}${detail ? `: ${detail}` : ''}`);
  }
}

async function run(): Promise<Record<string, unknown>> {
  // 1. Service-worker install / update / activation.
  const sw = new ServiceWorkerModel('1.0.0');
  check('sw.install.cache-name', sw.activeCacheName() === 'ssh-shell-1.0.0');

  const update = sw.onUpdateFound('1.1.0');
  check('sw.update.waiting', update.waiting && sw.pending !== null);
  check('sw.update.no-op-for-older', !sw.onUpdateFound('0.9.0').waiting);

  const existing = ['ssh-shell-1.0.0', 'ssh-shell-0.9.0', 'ssh-session-v0'];
  sw.registerCaches(existing);
  const purged = sw.activate();
  check('sw.activate.purges-stale', purged.length === 2 && purged.includes('ssh-shell-1.0.0'));
  check('sw.activate.cache-name', sw.activeCacheName() === 'ssh-shell-1.1.0');
  check('sw.activate.session-cache-memory-only', !sw.sessionCacheName().startsWith('ssh-shell'));

  // 2. Cache policy: app shell cacheable; session data never cached.
  const policy = new CachePolicy();
  check('cache.app-shell-cached', policy.shouldCache('/static/app.js'));
  check('cache.sensitive-token-forbidden', !policy.shouldCache('/session/token'));
  check('cache.session-data-forbidden', !policy.shouldCache('/data/terminal-stream'));

  // 3. Cache audit: clean caches have no violations; sensitive entries flag.
  const cleanEntries: CacheEntry[] = [
    { url: '/static/app.js', kind: 'app-shell', sensitive: false, bytes: 48192 },
    { url: '/static/app.css', kind: 'app-shell', sensitive: false, bytes: 12048 },
    { url: '/static/icon.svg', kind: 'app-shell', sensitive: false, bytes: 1024 },
  ];
  check('cache.audit.clean', policy.audit(cleanEntries).length === 0);

  const leakyEntries: CacheEntry[] = [
    ...cleanEntries,
    { url: '/session/token', kind: 'forbidden', sensitive: true, bytes: 64 },
    { url: '/data/terminal-stream', kind: 'session', sensitive: true, bytes: 4096 },
  ];
  const violations = policy.audit(leakyEntries);
  check('cache.audit.flags-secrets',
    violations.length === 3 &&
    violations.some((violation) => violation.startsWith('sensitive data cached')) &&
    violations.some((violation) => violation.startsWith('session data persisted')));

  // 4. Clear session data keeps the shell and drops every session entry.
  const afterClear = policy.clearSessionData(leakyEntries);
  check('cache.clear.keeps-shell', afterClear.length === 3 && afterClear.every((entry) => entry.kind === 'app-shell'));

  // 5. Offline never claims connectivity.
  const offline = new OfflinePolicy();
  check('offline.status-no-claim', offline.statusText('offline', 'ready') === 'Offline - not connected');
  check('offline.cannot-connect', !offline.canConnect('offline'));
  check('offline.can-connect-online', offline.canConnect('online'));

  // 6. Shell suspends a live session on offline; reconnects only online.
  const shell = createShell({ width: 1280, height: 720 });
  shell.selectHost('dev.example.com');
  await shell.connect('short-lived-token-offline');
  check('shell.online.ready', shell.phase === 'ready' && shell.canConnect());
  check('shell.online.status', shell.statusText() === 'Connected - encrypted session');
  shell.setConnectivity(false);
  check('shell.offline.suspended', shell.phase === 'offline');
  check('shell.offline.status-no-claim', shell.statusText() === 'Offline - not connected');
  check('shell.offline.cannot-connect', !shell.canConnect());
  shell.setConnectivity(true);
  check('shell.online.idle-reconnect', shell.phase === 'idle' && shell.canConnect());

  return {
    schema_version: 1,
    service_worker: {
      install: 'ssh-shell-1.0.0',
      update: { next: '1.1.0', waiting: true },
      activate_purged: purged,
      active_after: 'ssh-shell-1.1.0',
      session_cache_memory_only: 'ssh-session-v0',
    },
    cache_audit: {
      clean_entries: cleanEntries,
      clean_violations: 0,
      leaky_violations: violations,
      after_clear: afterClear.map((entry) => entry.url),
    },
    offline: {
      status_text_offline: 'Offline - not connected',
      can_connect_offline: false,
    },
  };
}

async function main(): Promise<void> {
  const snapshot = await run();
  if (failures > 0) {
    console.error(`PWA offline E2E failed with ${failures} failure(s)`);
    process.exit(1);
  }
  if (process.argv.includes('--write')) {
    const flagIndex = process.argv.indexOf('--write');
    const positional = process.argv
      .slice(flagIndex + 1)
      .find((arg) => !arg.startsWith('-'));
    const outDir = positional
      ? resolve(positional)
      : join(dirname(fileURLToPath(import.meta.url)), '..', 'pwa');
    mkdirSync(outDir, { recursive: true });
    const file = join(outDir, 'cache-audit.snapshot.json');
    writeFileSync(file, `${JSON.stringify(snapshot, null, 2)}\n`, 'utf8');
    console.log(`wrote ${file}`);
  }
  console.log('PWA offline E2E valid: service-worker upgrade purges stale caches, cache policy/audit hold no session secrets, clear-session-data keeps the shell, and offline never claims connectivity.');
}

void main();