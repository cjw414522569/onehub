// Critical-path E2E for the Web/PWA shell (T139): layout, connect handshake,
// terminal render, input encoding, resize, paste, and disconnect.

import { createShell } from '../src/index.ts';
import { encodeKeyXterm, encodePaste, Key } from '../src/input-adapter.ts';
import { layoutFor } from '../src/shell.ts';

let failures = 0;

function check(name: string, condition: boolean, detail = ''): void {
  if (condition) {
    console.log(`PASS ${name}`);
  } else {
    failures += 1;
    console.error(`FAIL ${name}${detail ? `: ${detail}` : ''}`);
  }
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join(' ');
}

// 1. Responsive layout: desktop and mobile critical paths.
const desktop = layoutFor({ width: 1440, height: 900 });
check('layout.desktop.sidebar', desktop.mode === 'desktop' && desktop.sidebarVisible);
check('layout.desktop.terminal', desktop.terminal.cols > 100 && desktop.terminal.rows > 40);
const mobile = layoutFor({ width: 390, height: 844 });
check('layout.mobile.sidebar-hidden', mobile.mode === 'mobile' && !mobile.sidebarVisible);
check('layout.mobile.terminal', mobile.terminal.cols > 30 && mobile.terminal.rows > 30);

// 2. Connect flow: host select -> gateway handshake -> ready.
const shell = createShell({ width: 1280, height: 720 });
check('connect.select-host', shell.selectHost('dev.example.com'));
check('connect.rejects-unknown-host', !shell.selectHost('nope.example'));
shell.connect('short-lived-token-1').then(async () => {
  check('connect.phase-ready', shell.phase === 'ready');
  check('connect.banner-rendered', shell.terminal().text().includes('Welcome to dev.example.com'));

  // 3. Input path: type "ls" + Enter; mock echoes a directory listing.
  for (const key of ['l', 's']) {
    shell.sendKey({ key: Key.Char, char: key, modifiers: { shift: false, alt: false, ctrl: false } });
  }
  shell.sendKey({ key: Key.Enter, modifiers: { shift: false, alt: false, ctrl: false } });
  check('input.output-rendered', shell.terminal().text().includes('file1') && shell.terminal().text().includes('file2'));

  // 4. Input adapter bytes mirror the native xterm contract.
  check('input.enter-cr', bytesToHex(encodeKeyXterm({ key: Key.Enter, modifiers: { shift: false, alt: false, ctrl: false } })) === '0d');
  check('input.ctrl-c', bytesToHex(encodeKeyXterm({ key: Key.Char, char: 'c', modifiers: { shift: false, alt: false, ctrl: true } })) === '03');
  check('input.arrow-up', bytesToHex(encodeKeyXterm({ key: Key.ArrowUp, modifiers: { shift: false, alt: false, ctrl: false } })) === '1b 5b 41');
  check('input.arrow-up-app', bytesToHex(encodeKeyXterm({ key: Key.ArrowUp, modifiers: { shift: false, alt: false, ctrl: false } }, true)) === '1b 4f 41');
  check('input.backspace', bytesToHex(encodeKeyXterm({ key: Key.Backspace, modifiers: { shift: false, alt: false, ctrl: false } })) === '7f');
  check('input.alt-x', bytesToHex(encodeKeyXterm({ key: Key.Char, char: 'x', modifiers: { shift: false, alt: true, ctrl: false } })) === '1b 78');
  check('input.paste-bracketed', bytesToHex(encodePaste('hi', true)) === '1b 5b 32 30 30 7e 68 69 1b 5b 32 30 31 7e');

  // 5. Resize preserves content (same width).
  const before = shell.terminal().text();
  shell.setViewport({ width: 1280, height: 900 });
  check('resize.preserves-content', shell.terminal().text().includes('file1'));

  // 6. Mobile switch hides the sidebar and keeps the session.
  shell.setViewport({ width: 390, height: 844 });
  check('mobile.shell-usable', shell.layout.mode === 'mobile' && !shell.layout.sidebarVisible);
  check('mobile.session-alive', shell.phase === 'ready');

  // 7. Disconnect closes the session.
  shell.disconnect();
  check('disconnect.closed', shell.phase === 'closed');

  if (failures > 0) {
    console.error(`E2E failed with ${failures} failure(s)`);
    process.exit(1);
  }
  console.log('web-shell E2E valid: responsive layout, gateway handshake, terminal render, input adapter, resize, mobile, and disconnect critical paths pass.');
});