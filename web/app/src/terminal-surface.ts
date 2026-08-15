// Terminal surface: a cell grid that renders snapshots produced by the Rust
// WASM bridge (T138). The surface is the browser-side display model; it
// consumes the same `Snapshot` shape (rows of cells with text) as native.

import type { Cell, Row, Snapshot, Size } from './types.ts';
import { emptyCell } from './types.ts';

/** The terminal surface model (DOM-free, deterministic). */
export class TerminalSurface {
  private readonly streamId: number;
  private grid: Cell[][];
  private cursorRow = 0;
  private cursorCol = 0;
  rows: number;
  cols: number;

  constructor(
    streamId: number,
    rows: number,
    cols: number,
  ) {
    this.streamId = streamId;
    this.rows = rows;
    this.cols = cols;
    this.grid = this.blank(rows, cols);
  }

  private blank(rows: number, cols: number): Cell[][] {
    return Array.from({ length: rows }, () =>
      Array.from({ length: cols }, () => emptyCell()),
    );
  }

  size(): Size {
    return { rows: this.rows, cols: this.cols };
  }

  cursor(): { row: number; col: number } {
    return { row: this.cursorRow, col: this.cursorCol };
  }

  /** Replaces the grid from a snapshot (same vectors as native). */
  applySnapshot(snapshot: Snapshot): void {
    this.rows = snapshot.rows.length;
    this.cols = snapshot.rows[0]?.cells.length ?? this.cols;
    this.grid = snapshot.rows.map((row: Row) =>
      row.cells.map((cell) => ({ ...cell })),
    );
    this.cursorRow = Math.min(snapshot.cursor.row, Math.max(this.rows - 1, 0));
    this.cursorCol = Math.min(snapshot.cursor.col, Math.max(this.cols - 1, 0));
  }

  /** Resizes, preserving content (rows added/removed at the bottom). */
  resize(rows: number, cols: number): void {
    if (rows === this.rows && cols === this.cols) return;
    const next = this.blank(rows, cols);
    for (let r = 0; r < Math.min(rows, this.rows); r += 1) {
      for (let c = 0; c < Math.min(cols, this.cols); c += 1) {
        next[r]![c] = this.grid[r]![c] ?? emptyCell();
      }
    }
    this.grid = next;
    this.rows = rows;
    this.cols = cols;
    this.cursorRow = Math.min(this.cursorRow, rows - 1);
    this.cursorCol = Math.min(this.cursorCol, cols - 1);
  }

  /** The visible text: rows joined by newline, trailing blank rows trimmed. */
  text(): string {
    const lines = this.grid.map((row) => row.map((cell) => cell.text).join('').replace(/\s+$/, ''));
    let last = lines.length - 1;
    while (last >= 0 && lines[last] === '') last -= 1;
    return lines.slice(0, last + 1).join('\n');
  }

  /** Cell at (row, col), or an empty cell when out of bounds. */
  cell(row: number, col: number): Cell {
    return this.grid[row]?.[col] ?? emptyCell();
  }

  /** Renders the surface as an SVG frame (screenshot matrix). */
  toSvg(engine: string, viewport: string, cellW = 9, cellH = 16): string {
    const w = this.cols * cellW;
    const h = this.rows * cellH;
    const rows = this.grid
      .map((row, r) => {
        const y = r * cellH;
        return row
          .map((cell, c) => {
            if (!cell.text) return '';
            const fg = cell.fg === 'default' ? '#d4d4d4' : cell.fg;
            const bg = cell.bg === 'default' ? '#1e1e1e' : cell.bg;
            const x = c * cellW;
            const bold = cell.bold ? ' font-weight="bold"' : '';
            return `<text x="${x}" y="${y + 13}"${bold} fill="${fg}">${escapeXml(cell.text)}</text>`;
          })
          .join('');
      })
      .join('');
    const cursor = this.cursorRow >= 0
      ? `<rect x="${this.cursorCol * cellW}" y="${this.cursorRow * cellH}" width="${cellW}" height="${cellH}" fill="#ffffff" opacity="0.35"/>`
      : '';
    return [
      '<?xml version="1.0" encoding="UTF-8"?>',
      `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">`,
      `<rect width="${w}" height="${h}" fill="#1e1e1e"/>`,
      `<!-- engine=${engine} viewport=${viewport} rows=${this.rows} cols=${this.cols} stream=${this.streamId} -->`,
      cursor,
      rows,
      '</svg>',
    ].join('\n');
  }
}

function escapeXml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}