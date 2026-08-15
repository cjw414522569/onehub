// Shared model types for the Web/PWA shell (T139).
// Mirrors the native core-protocol terminal snapshot shape so the surface
// consumes the same vectors the Rust WASM bridge (T138) produces.

/** One terminal cell. */
export interface Cell {
  /** Visible text; empty string for blank / wide-continuation cells. */
  text: string;
  /** ANSI 16-color foreground name (or hex). */
  fg: string;
  /** ANSI 16-color background name (or hex). */
  bg: string;
  /** Bold / bright style flag. */
  bold: boolean;
}

/** One row of cells. */
export interface Row {
  cells: Cell[];
}

/** A full terminal snapshot (bulk transport / recovery). */
export interface Snapshot {
  /** Stream identifier. */
  streamId: number;
  /** Monotonic per-stream sequence. */
  sequence: number;
  /** Visible rows. */
  rows: Row[];
  /** 0-based cursor position. */
  cursor: { row: number; col: number };
  /** Optional window title. */
  title?: string;
  /** Optional working directory (OSC 7). */
  workingDirectory?: string;
}

/** Grid dimensions. */
export interface Size {
  rows: number;
  cols: number;
}

/** A blank cell with default styling. */
export function emptyCell(): Cell {
  return { text: '', fg: 'default', bg: 'default', bold: false };
}

/** Builds a snapshot from plain text lines (testing / screenshots). */
export function snapshotFromText(
  streamId: number,
  lines: string[],
  rows: number,
  cols: number,
  cursor?: { row: number; col: number },
): Snapshot {
  const grid: Row[] = [];
  for (let r = 0; r < rows; r += 1) {
    const line = lines[r] ?? '';
    const cells: Cell[] = [];
    for (let c = 0; c < cols; c += 1) {
      cells.push({ text: line[c] ?? '', fg: 'default', bg: 'default', bold: false });
    }
    grid.push({ cells });
  }
  return {
    streamId,
    sequence: 1,
    rows: grid,
    cursor: cursor ?? { row: Math.min(lines.length, rows - 1), col: 0 },
  };
}