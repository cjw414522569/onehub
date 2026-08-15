// Web/PWA shell bootstrap (T139): factory that wires the responsive shell,
// terminal surface, and input adapter together.

import { MockGateway } from './gateway-transport.ts';
import type { Viewport } from './shell.ts';
import { ShellModel } from './shell.ts';

/** Default host list (demo; real hosts come from the host library). */
export const DEFAULT_HOSTS = ['dev.example.com', 'db.internal', 'prod-a.internal'];

/** Creates a shell with the mock gateway transport. */
export function createShell(viewport: Viewport, hosts: string[] = DEFAULT_HOSTS): ShellModel {
  return new ShellModel(viewport, hosts, () => new MockGateway());
}