// Input adapter: maps browser key events to byte sequences sent to the
// remote application, mirroring the native terminal-state `encode_key_xterm`
// contract (T060/T065) plus paste and focus encoders.

/** Modifier state of a key event. */
export interface Modifiers {
  shift: boolean;
  alt: boolean;
  ctrl: boolean;
}

/** Logical key ids for encoding. */
export const Key = {
  Char: 0,
  Enter: 1,
  Tab: 2,
  Backspace: 3,
  Escape: 4,
  ArrowUp: 5,
  ArrowDown: 6,
  ArrowLeft: 7,
  ArrowRight: 8,
  Home: 9,
  End: 10,
  PageUp: 11,
  PageDown: 12,
  Insert: 13,
  Delete: 14,
  Function: 15,
} as const;

/** A logical key for encoding. */
export type Key = (typeof Key)[keyof typeof Key];

/** A key event with modifiers (repeat defaults to 1). */
export interface KeyEvent {
  key: Key;
  /** The character for `Key.Char`. */
  char?: string;
  modifiers: Modifiers;
  repeat?: number;
}

const noMods: Modifiers = { shift: false, alt: false, ctrl: false };

function xtermBits(mods: Modifiers): number {
  return (mods.shift ? 1 : 0) | (mods.alt ? 2 : 0) | (mods.ctrl ? 4 : 0);
}

function bytes(...values: number[]): Uint8Array {
  return Uint8Array.from(values);
}

function utf8(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

function csi(body: string): Uint8Array {
  return utf8(`\u001b[${body}`);
}

/**
 * Encodes a key event under the xterm keyboard protocol, mirroring the
 * native `encode_key_xterm` behavior: printable keys pass as UTF-8,
 * Ctrl+letter becomes the control byte, Alt prefixes ESC, and the
 * application-cursor mode (?1) switches arrows to SS3 sequences.
 */
export function encodeKeyXterm(event: KeyEvent, appCursorKeys = false): Uint8Array {
  const mods = event.modifiers ?? noMods;
  const bits = xtermBits(mods);
  const modCode = bits !== 0 ? 1 + bits : 0;
  switch (event.key) {
    case Key.Char: {
      const ch = event.char ?? '';
      if (mods.ctrl && /^[a-z]$/.test(ch)) return bytes(ch.charCodeAt(0) & 0x1f);
      if (mods.alt) {
        const out = [0x1b, ...utf8(ch)];
        return Uint8Array.from(out);
      }
      return utf8(ch);
    }
    case Key.Enter:
      return modCode === 0 ? bytes(0x0d) : csi(`13;${modCode}~`);
    case Key.Tab:
      return modCode === 0 ? bytes(0x09) : csi(`9;${modCode}~`);
    case Key.Backspace:
      return modCode === 0 ? bytes(0x7f) : csi(`127;${modCode}~`);
    case Key.Escape:
      return bytes(0x1b);
    case Key.ArrowUp:
      if (appCursorKeys && modCode === 0) return bytes(0x1b, 0x4f, 0x41);
      return modCode === 0 ? csi('A') : csi(`1;${modCode}A`);
    case Key.ArrowDown:
      if (appCursorKeys && modCode === 0) return bytes(0x1b, 0x4f, 0x42);
      return modCode === 0 ? csi('B') : csi(`1;${modCode}B`);
    case Key.ArrowRight:
      if (appCursorKeys && modCode === 0) return bytes(0x1b, 0x4f, 0x43);
      return modCode === 0 ? csi('C') : csi(`1;${modCode}C`);
    case Key.ArrowLeft:
      if (appCursorKeys && modCode === 0) return bytes(0x1b, 0x4f, 0x44);
      return modCode === 0 ? csi('D') : csi(`1;${modCode}D`);
    case Key.Home:
      return modCode === 0 ? csi('H') : csi(`1;${modCode}H`);
    case Key.End:
      return modCode === 0 ? csi('F') : csi(`1;${modCode}F`);
    case Key.PageUp:
      return modCode === 0 ? csi('5~') : csi(`5;${modCode}~`);
    case Key.PageDown:
      return modCode === 0 ? csi('6~') : csi(`6;${modCode}~`);
    case Key.Insert:
      return modCode === 0 ? csi('2~') : csi(`2;${modCode}~`);
    case Key.Delete:
      return modCode === 0 ? csi('3~') : csi(`3;${modCode}~`);
    case Key.Function: {
      const n = event.char ? Number(event.char) : 1;
      return csi(`${n + 1}~`);
    }
    default:
      return utf8('');
  }
}

/** Pasted text, wrapped in bracketed-paste markers when ?2004 is active. */
export function encodePaste(text: string, bracketed: boolean): Uint8Array {
  if (!bracketed) return utf8(text);
  return utf8(`\u001b[200~${text}\u001b[201~`);
}

/** Focus report when ?1004 is active. */
export function encodeFocus(focused: boolean, focusEvents: boolean): Uint8Array | null {
  if (!focusEvents) return null;
  return focused ? utf8('\u001b[I') : utf8('\u001b[O');
}

/** Maps a DOM KeyboardEvent to a logical key event (browser adapter). */
export function fromDomEvent(event: {
  key: string;
  shiftKey: boolean;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}): KeyEvent | null {
  const modifiers: Modifiers = {
    shift: event.shiftKey,
    alt: event.altKey,
    ctrl: event.ctrlKey,
  };
  // Browser shortcuts (Cmd/Ctrl+key) are not terminal input.
  if (event.ctrlKey || event.metaKey) return null;
  switch (event.key) {
    case 'Enter':
      return { key: Key.Enter, modifiers };
    case 'Tab':
      return { key: Key.Tab, modifiers };
    case 'Backspace':
      return { key: Key.Backspace, modifiers };
    case 'Escape':
      return { key: Key.Escape, modifiers };
    case 'ArrowUp':
      return { key: Key.ArrowUp, modifiers };
    case 'ArrowDown':
      return { key: Key.ArrowDown, modifiers };
    case 'ArrowLeft':
      return { key: Key.ArrowLeft, modifiers };
    case 'ArrowRight':
      return { key: Key.ArrowRight, modifiers };
    case 'Home':
      return { key: Key.Home, modifiers };
    case 'End':
      return { key: Key.End, modifiers };
    case 'PageUp':
      return { key: Key.PageUp, modifiers };
    case 'PageDown':
      return { key: Key.PageDown, modifiers };
    case 'Insert':
      return { key: Key.Insert, modifiers };
    case 'Delete':
      return { key: Key.Delete, modifiers };
    default:
      if (event.key.length === 1) return { key: Key.Char, char: event.key, modifiers };
      if (/^F([1-9]|1[0-2])$/.test(event.key)) {
        return { key: Key.Function, char: event.key.slice(1), modifiers };
      }
      return null;
  }
}