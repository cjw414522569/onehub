// Minimal VT byte-stream parser for the browser shell critical path.
// Handles the text / CR / LF / BS / CSI cursor-and-erase subset used by the
// E2E critical path; the full contract parser lives in the Rust core (T062).

import type { Cell, Snapshot } from './types.ts';
import { emptyCell } from './types.ts';

/** A minimal streaming VT parser driving a cell grid. */
export class VtParser {
  private grid: Cell[][] = [];
  private cursorRow = 0;
  private cursorCol = 0;
  private pending: number[] = [];
  private sequence = 0;
  rows: number;
  cols: number;

  constructor(
    rows: number,
    cols: number,
  ) {
    this.rows = rows;
    this.cols = cols;
    this.grid = this.blank();
  }

  private blank(): Cell[][] {
    return Array.from({ length: this.rows }, () =>
      Array.from({ length: this.cols }, () => emptyCell()),
    );
  }

  /** Feeds bytes (UTF-8) and updates the grid. */
  feed(bytes: Uint8Array): void {
    for (const byte of bytes) this.feedByte(byte);
  }

  private feedByte(byte: number): void {
    if (this.pending.length > 0) {
      this.pending.push(byte);
      const text = new TextDecoder().decode(new Uint8Array(this.pending));
      if (!text.includes('\uFFFD') || this.pending.length >= 4) {
        for (const ch of text) if (ch !== '\uFFFD') this.putChar(ch);
        this.pending = [];
      }
      return;
    }
    if (byte === 0x1b) {
      this.pending = [byte];
      return;
    }
    if (byte === 0x0d) {
      this.cursorCol = 0;
      return;
    }
    if (byte === 0x0a) {
      if (this.cursorRow + 1 < this.rows) this.cursorRow += 1;
      return;
    }
    if (byte === 0x08) {
      this.cursorCol = Math.max(0, this.cursorCol - 1);
      return;
    }
    if (byte < 0x20) return;
    // Multi-byte UTF-8 lead byte.
    if (byte >= 0xc0) {
      this.pending = [byte];
      return;
    }
    this.putChar(String.fromCharCode(byte));
  }

  private putChar(ch: string): void {
    if (this.cursorCol >= this.cols) {
      this.cursorCol = 0;
      if (this.cursorRow + 1 < this.rows) this.cursorRow += 1;
    }
    const cell = this.grid[this.cursorRow]![this.cursorCol]!;
    cell.text = ch;
    this.cursorCol += 1;
  }

  /** The current snapshot, in the same shape the WASM bridge produces. */
  snapshot(streamId: number): Snapshot {
    this.sequence += 1;
    return {
      streamId,
      sequence: this.sequence,
      rows: this.grid.map((row) => ({ cells: row.map((cell) => ({ ...cell })) })),
      cursor: { row: this.cursorRow, col: this.cursorCol },
    };
  }
}