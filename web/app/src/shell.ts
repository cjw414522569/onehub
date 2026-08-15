// Responsive Web/PWA shell (T139): layout model, connection state machine,
// terminal surface wiring, and input adapter plumbing. DOM-free and
// deterministic so the critical paths are testable headlessly.

import type { GatewayTransport, SessionMessage } from './gateway-transport.ts';
import { SESSION_PROTOCOL_VERSION, dataMessage } from './gateway-transport.ts';
import type { KeyEvent } from './input-adapter.ts';
import { fromDomEvent, encodeKeyXterm, encodePaste } from './input-adapter.ts';
import { TerminalSurface } from './terminal-surface.ts';
import type { Size } from './types.ts';
import { VtParser } from './vt-parser.ts';

/** Shell connection phases. */
export type ShellPhase = 'idle' | 'connecting' | 'ready' | 'closed';

/** Browser viewport. */
export interface Viewport {
  width: number;
  height: number;
}

/** Responsive layout mode. */
export type LayoutMode = 'desktop' | 'mobile';

/** Layout decision for a viewport. */
export interface Layout {
  mode: LayoutMode;
  sidebarVisible: boolean;
  sidebarWidth: number;
  terminal: Size;
  statusBarVisible: boolean;
}

/** Desktop breakpoint (CSS px). */
export const DESKTOP_BREAKPOINT = 1024;
/** Sidebar width on desktop. */
export const SIDEBAR_WIDTH = 240;
/** Terminal cell metrics. */
export const CELL_W = 9;
export const CELL_H = 16;
/** Status bar height. */
export const STATUS_BAR_H = 24;

/** Computes the responsive layout for a viewport. */
export function layoutFor(viewport: Viewport): Layout {
  const mode: LayoutMode = viewport.width >= DESKTOP_BREAKPOINT ? 'desktop' : 'mobile';
  const sidebarVisible = mode === 'desktop';
  const sidebarWidth = sidebarVisible ? SIDEBAR_WIDTH : 0;
  const statusBarVisible = true;
  const usableW = Math.max(40, viewport.width - sidebarWidth - 16);
  const usableH = Math.max(12, viewport.height - STATUS_BAR_H - 16);
  return {
    mode,
    sidebarVisible,
    sidebarWidth,
    terminal: { cols: Math.floor(usableW / CELL_W), rows: Math.floor(usableH / CELL_H) },
    statusBarVisible,
  };
}

/**
 * The browser shell: owns the terminal surface, the byte-stream parser, and
 * the transport; drives the connect -> ready -> data -> disconnect flow.
 */
export class ShellModel {
  phase: ShellPhase = 'idle';
  viewport: Viewport;
  layout: Layout;
  selectedHost: string | null = null;
  title = '';

  readonly hosts: string[];
  private readonly transportFactory: () => GatewayTransport;
  private surface: TerminalSurface;
  private parser: VtParser;
  private transport: GatewayTransport | null = null;
  private sequence = 0;
  private sessionToken: string | null = null;

  constructor(
    viewport: Viewport,
    hosts: string[],
    transportFactory: () => GatewayTransport,
    streamId = 1,
  ) {
    this.hosts = hosts;
    this.transportFactory = transportFactory;
    this.viewport = viewport;
    this.layout = layoutFor(viewport);
    const size = this.layout.terminal;
    this.surface = new TerminalSurface(streamId, size.rows, size.cols);
    this.parser = new VtParser(size.rows, size.cols);
  }

  /** The terminal surface (for rendering / tests). */
  terminal(): TerminalSurface {
    return this.surface;
  }

  /** Applies a new viewport: recompute layout and resize the surface. */
  setViewport(viewport: Viewport): void {
    this.viewport = viewport;
    this.layout = layoutFor(viewport);
    this.surface.resize(this.layout.terminal.rows, this.layout.terminal.cols);
    this.parser.rows = this.layout.terminal.rows;
    this.parser.cols = this.layout.terminal.cols;
  }

  /** Selects a host from the list. */
  selectHost(host: string): boolean {
    if (!this.hosts.includes(host)) return false;
    this.selectedHost = host;
    return true;
  }

  /**
   * Connects through the gateway with a short-lived token (T137): performs
   * the versioned handshake and waits for the session to reach ready.
   */
  async connect(token: string): Promise<void> {
    if (!this.selectedHost) throw new Error('no host selected');
    this.phase = 'connecting';
    const transport = this.transportFactory();
    this.transport = transport;
    transport.onMessage((message) => this.onGatewayMessage(message));
    await transport.connect(this.selectedHost, token);
    this.sessionToken = token;
    // Authenticate with the short-lived token.
    transport.send({
      version: SESSION_PROTOCOL_VERSION,
      kind: 'auth',
      sequence: ++this.sequence,
      cancel: false,
      backpressure: false,
      payload: new TextEncoder().encode(token),
    });
  }

  /** Sends a key event to the remote application. */
  sendKey(event: KeyEvent): void {
    if (this.phase !== 'ready' || !this.transport) return;
    const bytes = encodeKeyXterm(event);
    if (bytes.length === 0) return;
    this.transport.send(dataMessage(++this.sequence, bytes));
  }

  /** Sends pasted text (bracketed-paste aware). */
  sendPaste(text: string, bracketed: boolean): void {
    if (this.phase !== 'ready' || !this.transport) return;
    this.transport.send(dataMessage(++this.sequence, encodePaste(text, bracketed)));
  }

  /** Disconnects and closes the session. */
  disconnect(): void {
    if (this.transport) {
      this.transport.send({
        version: SESSION_PROTOCOL_VERSION,
        kind: 'close',
        sequence: ++this.sequence,
        cancel: false,
        backpressure: false,
        payload: new Uint8Array(),
      });
      this.transport.close();
    }
    this.phase = 'closed';
    this.sessionToken = null;
  }

  private onGatewayMessage(message: SessionMessage): void {
    switch (message.kind) {
      case 'capabilities':
        // Negotiation is complete: the session is ready.
        this.phase = 'ready';
        break;
      case 'data':
        this.parser.feed(message.payload);
        this.surface.applySnapshot(this.parser.snapshot(1));
        break;
      case 'backpressure':
        // Backpressure flag handling is a no-op for the shell model.
        break;
      default:
        break;
    }
  }
}

/** Browser input binding: adapts a DOM-like key event into terminal bytes. */
export function bindInput(shell: ShellModel, source: {
  addEventListener(type: 'keydown', listener: (event: {
    key: string;
    shiftKey: boolean;
    altKey: boolean;
    ctrlKey: boolean;
    metaKey: boolean;
    preventDefault: () => void;
  }) => void): void;
}): void {
  source.addEventListener('keydown', (event) => {
    const logical = fromDomEvent(event);
    if (!logical) return;
    event.preventDefault();
    shell.sendKey(logical);
  });
}