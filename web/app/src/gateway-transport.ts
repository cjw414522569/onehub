// Gateway transport contract (T139): the browser side of the versioned
// session protocol (T135). Messages mirror `SessionMessage` framing:
// version / kind / sequence / cancel / backpressure / payload.

/** The session protocol version (matches the Rust gateway). */
export const SESSION_PROTOCOL_VERSION = 1;

/** Message kinds in the versioned session protocol. */
export type MessageType =
  | 'hello'
  | 'auth'
  | 'capabilities'
  | 'data'
  | 'cancel'
  | 'backpressure'
  | 'close'
  | 'resume';

/** A versioned session message (mirrors the Rust `SessionMessage`). */
export interface SessionMessage {
  version: number;
  kind: MessageType;
  sequence: number;
  cancel: boolean;
  backpressure: boolean;
  payload: Uint8Array;
}

/** A framed data message (helper). */
export function dataMessage(sequence: number, payload: Uint8Array): SessionMessage {
  return {
    version: SESSION_PROTOCOL_VERSION,
    kind: 'data',
    sequence,
    cancel: false,
    backpressure: false,
    payload,
  };
}

/** The transport interface the shell depends on (WebSocket in production). */
export interface GatewayTransport {
  connect(host: string, token: string): Promise<void>;
  send(message: SessionMessage): void;
  close(): void;
  onMessage(callback: (message: SessionMessage) => void): void;
}

/**
 * A deterministic in-memory gateway for E2E: performs the versioned
 * handshake (hello -> auth -> capabilities -> ready) and echoes canned
 * output for the critical-path test.
 */
export class MockGateway implements GatewayTransport {
  private callback: ((message: SessionMessage) => void) | null = null;
  private sequence = 0;
  private ready = false;
  private host = '';
  private inputBuffer: number[] = [];
  connected = false;

  onMessage(callback: (message: SessionMessage) => void): void {
    this.callback = callback;
  }

  async connect(host: string, _token: string): Promise<void> {
    this.host = host;
    this.connected = true;
    this.emit({
      version: SESSION_PROTOCOL_VERSION,
      kind: 'hello',
      sequence: ++this.sequence,
      cancel: false,
      backpressure: false,
      payload: new TextEncoder().encode('SSH-GATEWAY/1.0'),
    });
  }

  send(message: SessionMessage): void {
    if (!this.connected || message.version !== SESSION_PROTOCOL_VERSION) return;
    if (message.kind === 'auth') {
      this.emit({
        version: SESSION_PROTOCOL_VERSION,
        kind: 'capabilities',
        sequence: ++this.sequence,
        cancel: false,
        backpressure: false,
        payload: new TextEncoder().encode('shell,sftp,forward,resume'),
      });
      this.ready = true;
      // Canned banner the moment the session is ready.
      const banner = `Welcome to ${this.host}\r\nLast login: Fri Aug 15 11:00:00 2026\r\n$ `;
      this.emit(dataMessage(++this.sequence, new TextEncoder().encode(banner)));
      return;
    }
    if (message.kind === 'data' && this.ready) {
      this.inputBuffer.push(...message.payload);
      // Echo only on Enter (CR), like a real remote shell.
      if (this.inputBuffer.includes(0x0d)) {
        const input = new TextDecoder()
          .decode(Uint8Array.from(this.inputBuffer))
          .replace(/\r|\n/g, '')
          .trim();
        this.inputBuffer = [];
        const reply = `$ ${input}\r\nfile1\r\nfile2\r\n$ `;
        this.emit(dataMessage(++this.sequence, new TextEncoder().encode(reply)));
      }
      return;
    }
    if (message.kind === 'close') {
      this.connected = false;
      this.ready = false;
    }
  }

  close(): void {
    this.connected = false;
    this.ready = false;
  }

  private emit(message: SessionMessage): void {
    this.callback?.(message);
  }
}