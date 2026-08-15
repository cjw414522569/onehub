// Page objects for the six-platform E2E smoke matrix (T153).
//
// Each platform page object drives the same critical journey — launch,
// select host, connect (gateway token), type a command, verify output,
// disconnect — against the deterministic fake gateway. The page objects are
// model-level (DOM-free) so the smoke matrix runs headlessly and stably.

import type { Viewport } from '../../web/app/src/shell.ts';
import { ShellModel } from '../../web/app/src/shell.ts';
import { Key } from '../../web/app/src/input-adapter.ts';
import { createShell } from '../../web/app/src/index.ts';

/** The critical journey a page object must pass. */
export interface JourneyReport {
  platform: string;
  passed: boolean;
  checks: string[];
}

/** A platform page object. */
export interface PageObject {
  readonly platform: string;
  run(token: string): Promise<JourneyReport>;
}

/** The shared shell page object. */
export class ShellPageObject implements PageObject {
  readonly platform: string;
  private readonly viewport: Viewport;
  private readonly host: string;

  constructor(platform: string, viewport: Viewport, host: string) {
    this.platform = platform;
    this.viewport = viewport;
    this.host = host;
  }

  async run(token: string): Promise<JourneyReport> {
    const checks: string[] = [];
    const shell = createShell(this.viewport);
    checks.push(`launch(${this.platform})=ok`);

    if (!shell.selectHost(this.host)) {
      return { platform: this.platform, passed: false, checks: [...checks, 'selectHost=failed'] };
    }
    checks.push('selectHost=ok');

    // Connect through the gateway with the env-provided token (await the
    // handshake so the session reaches ready before typing).
    await shell.connect(token);
    checks.push(`connect=${shell.phase}`);

    // Type a command and verify the echoed listing.
    for (const ch of ['l', 's']) {
      shell.sendKey({ key: Key.Char, char: ch, modifiers: { shift: false, alt: false, ctrl: false } });
    }
    shell.sendKey({ key: Key.Enter, modifiers: { shift: false, alt: false, ctrl: false } });
    const text = shell.terminal().text();
    checks.push(`banner=${text.includes('Welcome to')}`);
    checks.push(`output=${text.includes('file1') && text.includes('file2')}`);

    shell.disconnect();
    checks.push(`disconnect=${shell.phase}`);
    return { platform: this.platform, passed: checks.every((c) => !c.endsWith('=false') && !c.includes('=failed')), checks };
  }
}

/** The six-platform matrix. */
export function platformMatrix(): PageObject[] {
  return [
    new ShellPageObject('windows', { width: 1440, height: 900 }, 'dev.example.com'),
    new ShellPageObject('macos', { width: 1440, height: 900 }, 'dev.example.com'),
    new ShellPageObject('linux', { width: 1440, height: 900 }, 'dev.example.com'),
    new ShellPageObject('ios', { width: 390, height: 844 }, 'dev.example.com'),
    new ShellPageObject('android', { width: 412, height: 915 }, 'dev.example.com'),
    new ShellPageObject('web', { width: 1280, height: 720 }, 'dev.example.com'),
  ];
}