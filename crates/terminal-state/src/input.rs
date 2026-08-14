//! Input protocol encoders and keyboard negotiation (T068).
//!
//! The terminal sends bytes *to* the remote application: pasted text
//! (wrapped in bracketed-paste markers when ?2004 is active), focus-change
//! reports (?1004), mouse reports (?1000/?1002/?1003 with X10 or SGR
//! encoding), and key events (xterm / modifyOtherKeys / kitty keyboard
//! protocol). The encoders are pure and deterministic so input-protocol
//! replay tests can compare exact bytes.

/// Modifier state of an input event (kitty modifier bit positions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Alt.
    pub alt: bool,
    /// Control.
    pub ctrl: bool,
    /// Super / Windows / Command.
    pub super_: bool,
    /// Hyper.
    pub hyper: bool,
    /// Meta.
    pub meta: bool,
    /// Caps lock.
    pub caps_lock: bool,
    /// Num lock.
    pub num_lock: bool,
}

impl Modifiers {
    /// Kitty keyboard protocol modifier bitmask.
    pub fn kitty_bits(self) -> u16 {
        let mut bits = 0u16;
        if self.shift {
            bits |= 1;
        }
        if self.alt {
            bits |= 2;
        }
        if self.ctrl {
            bits |= 4;
        }
        if self.super_ {
            bits |= 8;
        }
        if self.hyper {
            bits |= 16;
        }
        if self.meta {
            bits |= 32;
        }
        if self.caps_lock {
            bits |= 64;
        }
        if self.num_lock {
            bits |= 128;
        }
        bits
    }

    /// xterm modifyOtherKeys / legacy mouse modifier bits (shift=1, alt=2,
    /// ctrl=4 -> encoded as 1/2/4).
    pub fn xterm_bits(self) -> u8 {
        let mut bits = 0u8;
        if self.shift {
            bits |= 1;
        }
        if self.alt {
            bits |= 2;
        }
        if self.ctrl {
            bits |= 4;
        }
        bits
    }
}

/// A logical key for encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable character.
    Char(char),
    /// Enter.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
    /// Arrow keys.
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Insert.
    Insert,
    /// Delete.
    Delete,
    /// Function key F1..=F12.
    Function(u8),
}

/// A key event with modifiers and repeat count (kitty event suffix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    /// The key.
    pub key: Key,
    /// Held modifiers.
    pub modifiers: Modifiers,
    /// Repeat count (1 = press).
    pub repeat: u8,
}

impl KeyEvent {
    /// A single press with no modifiers.
    pub fn press(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::default(),
            repeat: 1,
        }
    }
}

/// Mouse button for a mouse event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Middle button.
    Middle,
    /// Right button.
    Right,
    /// No button (motion).
    None,
}

/// Mouse event action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    /// Button pressed.
    Press,
    /// Button released.
    Release,
    /// Pointer moved.
    Motion,
    /// Wheel scrolled up.
    WheelUp,
    /// Wheel scrolled down.
    WheelDown,
}

/// A mouse event (1-based row/column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    /// 1-based column.
    pub col: u16,
    /// 1-based row.
    pub row: u16,
    /// Button.
    pub button: MouseButton,
    /// Action.
    pub action: MouseAction,
    /// Held modifiers.
    pub modifiers: Modifiers,
}

/// Mouse tracking mode (?1000/?1002/?1003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseMode {
    /// No mouse reporting.
    #[default]
    Off,
    /// ?1000 — report button press/release.
    Buttons,
    /// ?1002 — report drags too.
    Drag,
    /// ?1003 — report all motion.
    Motion,
}

/// Mouse coordinate encoding (?1006 and default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseEncoding {
    /// Legacy X10 encoding.
    #[default]
    X10,
    /// SGR encoding (?1006).
    Sgr,
}

/// Keyboard protocol negotiated with the remote application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardProtocol {
    /// Plain xterm sequences.
    #[default]
    Xterm,
    /// xterm modifyOtherKeys (`CSI 27;mod;code~`).
    ModifyOtherKeys,
    /// kitty keyboard protocol (`CSI code:mod:event u`).
    Kitty,
}

impl KeyboardProtocol {
    /// The probe sequence the terminal sends to detect kitty support
    /// (`CSI ? u`).
    pub fn kitty_probe() -> &'static str {
        "\x1b[? u"
    }

    /// Selects the protocol from the remote's reply to the kitty probe.
    /// A `CSI ? 1;2 u` reply means kitty is supported; otherwise fall back
    /// to xterm.
    pub fn from_kitty_reply(reply: Option<&str>) -> Self {
        match reply {
            Some(reply) if reply.starts_with("\x1b[? 1") => KeyboardProtocol::Kitty,
            _ => KeyboardProtocol::Xterm,
        }
    }
}

/// Encodes pasted text. When `bracketed` (?2004) is active the text is
/// wrapped in `CSI 200~` / `CSI 201~` so the remote can distinguish paste
/// from typed input.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x1b[200~");
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
        bytes
    } else {
        text.as_bytes().to_vec()
    }
}

/// Encodes a focus-change report (?1004). Returns `None` when focus events
/// are disabled.
pub fn encode_focus(focused: bool, focus_events: bool) -> Option<Vec<u8>> {
    if !focus_events {
        return None;
    }
    Some(if focused {
        b"\x1b[I".to_vec()
    } else {
        b"\x1b[O".to_vec()
    })
}

fn mouse_modifier_bits(modifiers: &Modifiers) -> u8 {
    let mut bits = 0u8;
    if modifiers.shift {
        bits |= 4;
    }
    if modifiers.alt || modifiers.meta {
        bits |= 8;
    }
    if modifiers.ctrl {
        bits |= 16;
    }
    bits
}

fn mouse_button_code(event: &MouseEvent) -> u8 {
    let base = match (event.action, event.button) {
        (MouseAction::Press | MouseAction::Release, MouseButton::Left) => 0,
        (MouseAction::Press | MouseAction::Release, MouseButton::Middle) => 1,
        (MouseAction::Press | MouseAction::Release, MouseButton::Right) => 2,
        (MouseAction::Press | MouseAction::Release, MouseButton::None) => 32,
        (MouseAction::Motion, _) => 32,
        (MouseAction::WheelUp, _) => 64,
        (MouseAction::WheelDown, _) => 65,
    };
    let modifiers = mouse_modifier_bits(&event.modifiers);
    base | modifiers
}

/// Encodes a mouse event according to the active mode/encoding. Returns
/// `None` when mouse reporting is off.
pub fn encode_mouse(
    event: &MouseEvent,
    mode: MouseMode,
    encoding: MouseEncoding,
) -> Option<Vec<u8>> {
    if mode == MouseMode::Off {
        return None;
    }
    if mode == MouseMode::Buttons && event.action == MouseAction::Motion {
        return None;
    }
    let code = mouse_button_code(event);
    match encoding {
        MouseEncoding::Sgr => {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"\x1b[<");
            bytes.extend_from_slice(code.to_string().as_bytes());
            bytes.push(b';');
            bytes.extend_from_slice(event.col.to_string().as_bytes());
            bytes.push(b';');
            bytes.extend_from_slice(event.row.to_string().as_bytes());
            bytes.push(if event.action == MouseAction::Release {
                b'm'
            } else {
                b'M'
            });
            Some(bytes)
        }
        MouseEncoding::X10 => {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"\x1b[M");
            bytes.push(0x20 + code);
            bytes.push(0x21 + (event.col.min(0x7ff) as u8 & 0x7f));
            bytes.push(0x21 + (event.row.min(0x7ff) as u8 & 0x7f));
            Some(bytes)
        }
    }
}

fn kitty_code(key: Key) -> u32 {
    match key {
        Key::Char(ch) => ch as u32,
        Key::Enter => 13,
        Key::Tab => 9,
        Key::Backspace => 127,
        Key::Escape => 27,
        Key::ArrowUp => 57421,
        Key::ArrowDown => 57422,
        Key::ArrowLeft => 57423,
        Key::ArrowRight => 57424,
        Key::Home => 57425,
        Key::End => 57426,
        Key::PageUp => 57427,
        Key::PageDown => 57428,
        Key::Insert => 57429,
        Key::Delete => 57430,
        Key::Function(n) => 57377 + u32::from(n.saturating_sub(1).min(11)),
    }
}

/// Encodes a key event under the given keyboard protocol. `app_cursor_keys`
/// (?1) and `app_keypad` (?66) select application-mode sequences.
pub fn encode_key(
    event: &KeyEvent,
    protocol: KeyboardProtocol,
    app_cursor_keys: bool,
    app_keypad: bool,
) -> Vec<u8> {
    let modifiers = event.modifiers;
    match protocol {
        KeyboardProtocol::Kitty => {
            let mod_bits = modifiers.kitty_bits();
            // Unmodified printable keys travel as UTF-8; everything else uses
            // `CSI code:mod[:repeat] u` (kitty includes a literal space).
            if let Key::Char(ch) = event.key {
                if mod_bits == 0 && event.repeat == 1 {
                    return ch.to_string().into_bytes();
                }
            }
            let code = kitty_code(event.key);
            let mut out = String::from("\x1b[");
            out.push_str(&code.to_string());
            if mod_bits != 0 || event.repeat > 1 || event.key == Key::Escape {
                out.push(':');
                out.push_str(&mod_bits.to_string());
            }
            if event.repeat > 1 {
                out.push(':');
                out.push_str(&event.repeat.to_string());
            }
            out.push(' ');
            out.push('u');
            out.into_bytes()
        }
        KeyboardProtocol::ModifyOtherKeys => {
            if let Key::Char(ch) = event.key {
                if modifiers.xterm_bits() != 0 {
                    let code = ch as u32;
                    let modifier = 1 + modifiers.xterm_bits();
                    return format!("\x1b[27;{modifier};{code}~").into_bytes();
                }
            }
            encode_key_xterm(event, app_cursor_keys, app_keypad)
        }
        KeyboardProtocol::Xterm => encode_key_xterm(event, app_cursor_keys, app_keypad),
    }
}

fn encode_key_xterm(event: &KeyEvent, app_cursor_keys: bool, _app_keypad: bool) -> Vec<u8> {
    let modifiers = event.modifiers;
    // xterm CSI modifier parameter: 1 + shift(1) + alt(2) + ctrl(4); 0 when
    // no modifiers are held so plain keys emit their simple sequences.
    let has_mods = modifiers.xterm_bits() != 0;
    let mod_code = if has_mods {
        1 + modifiers.xterm_bits()
    } else {
        0
    };

    let csi: Option<String> = match event.key {
        Key::Char(ch) => {
            if modifiers.ctrl && ch.is_ascii_lowercase() {
                // Control characters for letters.
                return vec![(ch as u8) & 0x1f];
            }
            if modifiers.alt {
                let mut bytes = vec![0x1b];
                bytes.extend_from_slice(ch.to_string().as_bytes());
                return bytes;
            }
            return ch.to_string().into_bytes();
        }
        Key::Enter => {
            if mod_code == 0 {
                return b"\r".to_vec();
            }
            Some(format!("13;{}~", mod_code))
        }
        Key::Tab => {
            if mod_code == 0 {
                return b"\t".to_vec();
            }
            Some(format!("9;{}~", mod_code))
        }
        Key::Backspace => {
            if mod_code == 0 {
                return b"\x7f".to_vec();
            }
            Some(format!("127;{}~", mod_code))
        }
        Key::Escape => return b"\x1b".to_vec(),
        Key::ArrowUp => {
            if app_cursor_keys && mod_code == 0 {
                return b"\x1bOA".to_vec();
            }
            Some(if mod_code == 0 {
                "A".to_owned()
            } else {
                format!("1;{}A", mod_code)
            })
        }
        Key::ArrowDown => {
            if app_cursor_keys && mod_code == 0 {
                return b"\x1bOB".to_vec();
            }
            Some(if mod_code == 0 {
                "B".to_owned()
            } else {
                format!("1;{}B", mod_code)
            })
        }
        Key::ArrowRight => {
            if app_cursor_keys && mod_code == 0 {
                return b"\x1bOC".to_vec();
            }
            Some(if mod_code == 0 {
                "C".to_owned()
            } else {
                format!("1;{}C", mod_code)
            })
        }
        Key::ArrowLeft => {
            if app_cursor_keys && mod_code == 0 {
                return b"\x1bOD".to_vec();
            }
            Some(if mod_code == 0 {
                "D".to_owned()
            } else {
                format!("1;{}D", mod_code)
            })
        }
        Key::Home => {
            if app_cursor_keys && mod_code == 0 {
                return b"\x1bOH".to_vec();
            }
            Some(if mod_code == 0 {
                "H".to_owned()
            } else {
                format!("1;{}H", mod_code)
            })
        }
        Key::End => {
            if app_cursor_keys && mod_code == 0 {
                return b"\x1bOF".to_vec();
            }
            Some(if mod_code == 0 {
                "F".to_owned()
            } else {
                format!("1;{}F", mod_code)
            })
        }
        Key::PageUp => Some(if mod_code == 0 {
            "5~".to_owned()
        } else {
            format!("5;{}~", mod_code)
        }),
        Key::PageDown => Some(if mod_code == 0 {
            "6~".to_owned()
        } else {
            format!("6;{}~", mod_code)
        }),
        Key::Insert => Some(if mod_code == 0 {
            "2~".to_owned()
        } else {
            format!("2;{}~", mod_code)
        }),
        Key::Delete => Some(if mod_code == 0 {
            "3~".to_owned()
        } else {
            format!("3;{}~", mod_code)
        }),
        Key::Function(n) => {
            let code = match n {
                1 => 11,
                2 => 12,
                3 => 13,
                4 => 14,
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                _ => 11,
            };
            Some(if mod_code == 0 {
                format!("{code}~")
            } else {
                format!("{code};{mod_code}~")
            })
        }
    };
    match csi {
        Some(csi) => format!("\x1b[{csi}").into_bytes(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        encode_focus, encode_key, encode_mouse, encode_paste, Key, KeyEvent, KeyboardProtocol,
        Modifiers, MouseAction, MouseButton, MouseEncoding, MouseEvent, MouseMode,
    };

    fn modifiers(shift: bool, ctrl: bool, alt: bool) -> Modifiers {
        Modifiers {
            shift,
            ctrl,
            alt,
            ..Modifiers::default()
        }
    }

    #[test]
    fn paste_is_wrapped_only_when_bracketed() {
        assert_eq!(encode_paste("hello", false), b"hello");
        assert_eq!(encode_paste("hello", true), b"\x1b[200~hello\x1b[201~");
    }

    #[test]
    fn focus_events_report_only_when_enabled() {
        assert_eq!(encode_focus(true, false), None);
        assert_eq!(encode_focus(true, true), Some(b"\x1b[I".to_vec()));
        assert_eq!(encode_focus(false, true), Some(b"\x1b[O".to_vec()));
    }

    #[test]
    fn sgr_mouse_encoding() {
        let event = MouseEvent {
            col: 10,
            row: 5,
            button: MouseButton::Left,
            action: MouseAction::Press,
            modifiers: Modifiers::default(),
        };
        assert_eq!(
            encode_mouse(&event, MouseMode::Buttons, MouseEncoding::Sgr),
            Some(b"\x1b[<0;10;5M".to_vec())
        );
        let release = MouseEvent {
            action: MouseAction::Release,
            ..event
        };
        assert_eq!(
            encode_mouse(&release, MouseMode::Buttons, MouseEncoding::Sgr),
            Some(b"\x1b[<0;10;5m".to_vec())
        );
        // Ctrl modifier adds 16 to the button code (SGR: 0 + 16).
        let ctrl = MouseEvent {
            modifiers: modifiers(false, true, false),
            ..event
        };
        assert_eq!(
            encode_mouse(&ctrl, MouseMode::Buttons, MouseEncoding::Sgr),
            Some(b"\x1b[<16;10;5M".to_vec())
        );
        assert_eq!(
            encode_mouse(&event, MouseMode::Off, MouseEncoding::Sgr),
            None
        );
    }

    #[test]
    fn x10_mouse_encoding() {
        let event = MouseEvent {
            col: 1,
            row: 1,
            button: MouseButton::Left,
            action: MouseAction::Press,
            modifiers: Modifiers::default(),
        };
        assert_eq!(
            encode_mouse(&event, MouseMode::Buttons, MouseEncoding::X10),
            Some(b"\x1b[M \"\"".to_vec())
        );
        // Motion only in Motion mode.
        let motion = MouseEvent {
            action: MouseAction::Motion,
            ..event
        };
        assert_eq!(
            encode_mouse(&motion, MouseMode::Buttons, MouseEncoding::X10),
            None
        );
        assert_eq!(
            encode_mouse(&motion, MouseMode::Motion, MouseEncoding::X10),
            Some(b"\x1b[M@\"\"".to_vec())
        );
    }

    #[test]
    fn xterm_key_encoding() {
        assert_eq!(
            encode_key(
                &KeyEvent::press(Key::Char('a')),
                KeyboardProtocol::Xterm,
                false,
                false
            ),
            b"a"
        );
        assert_eq!(
            encode_key(
                &KeyEvent::press(Key::Enter),
                KeyboardProtocol::Xterm,
                false,
                false
            ),
            b"\r"
        );
        assert_eq!(
            encode_key(
                &KeyEvent::press(Key::ArrowUp),
                KeyboardProtocol::Xterm,
                false,
                false
            ),
            b"\x1b[A"
        );
        assert_eq!(
            encode_key(
                &KeyEvent::press(Key::ArrowUp),
                KeyboardProtocol::Xterm,
                true,
                false
            ),
            b"\x1bOA"
        );
        // Ctrl+letter becomes a control byte.
        let ctrl_a = KeyEvent {
            key: Key::Char('a'),
            modifiers: modifiers(false, true, false),
            repeat: 1,
        };
        assert_eq!(
            encode_key(&ctrl_a, KeyboardProtocol::Xterm, false, false),
            b"\x01"
        );
        // Shift+arrow uses CSI 1;2A.
        let shift_up = KeyEvent {
            key: Key::ArrowUp,
            modifiers: modifiers(true, false, false),
            repeat: 1,
        };
        assert_eq!(
            encode_key(&shift_up, KeyboardProtocol::Xterm, false, false),
            b"\x1b[1;2A"
        );
    }

    #[test]
    fn modify_other_keys_encoding() {
        let ctrl_shift_x = KeyEvent {
            key: Key::Char('x'),
            modifiers: modifiers(true, true, false),
            repeat: 1,
        };
        assert_eq!(
            encode_key(
                &ctrl_shift_x,
                KeyboardProtocol::ModifyOtherKeys,
                false,
                false
            ),
            b"\x1b[27;6;120~"
        );
    }

    #[test]
    fn kitty_key_encoding() {
        // Plain 'a' with kitty is still UTF-8.
        assert_eq!(
            encode_key(
                &KeyEvent::press(Key::Char('a')),
                KeyboardProtocol::Kitty,
                false,
                false
            ),
            b"a"
        );
        // Ctrl+Alt+'a' -> CSI 97:6 u (ctrl=4, alt=2).
        let ca = KeyEvent {
            key: Key::Char('a'),
            modifiers: modifiers(false, true, true),
            repeat: 1,
        };
        assert_eq!(
            encode_key(&ca, KeyboardProtocol::Kitty, false, false),
            b"\x1b[97:6 u"
        );
        // Up arrow -> CSI 57421 u.
        assert_eq!(
            encode_key(
                &KeyEvent::press(Key::ArrowUp),
                KeyboardProtocol::Kitty,
                false,
                false
            ),
            b"\x1b[57421 u"
        );
        // Shift+Up -> CSI 57421:1 u.
        let shift_up = KeyEvent {
            key: Key::ArrowUp,
            modifiers: modifiers(true, false, false),
            repeat: 1,
        };
        assert_eq!(
            encode_key(&shift_up, KeyboardProtocol::Kitty, false, false),
            b"\x1b[57421:1 u"
        );
        // Repeat -> CSI 97:0:3 u.
        let repeated = KeyEvent {
            key: Key::Char('a'),
            modifiers: Modifiers::default(),
            repeat: 3,
        };
        assert_eq!(
            encode_key(&repeated, KeyboardProtocol::Kitty, false, false),
            b"\x1b[97:0:3 u"
        );
    }

    #[test]
    fn keyboard_protocol_negotiation() {
        assert_eq!(
            KeyboardProtocol::from_kitty_reply(None),
            KeyboardProtocol::Xterm
        );
        assert_eq!(
            KeyboardProtocol::from_kitty_reply(Some("\x1b[? 1;2 u")),
            KeyboardProtocol::Kitty
        );
        assert_eq!(
            KeyboardProtocol::from_kitty_reply(Some("\x1b[? 0 u")),
            KeyboardProtocol::Xterm
        );
        assert_eq!(KeyboardProtocol::kitty_probe(), "\x1b[? u");
    }
}
