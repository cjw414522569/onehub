//! Physical keyboard, IME, modifier combinations, and configurable shortcuts
//! (T108).
//!
//! [`KeyMap::normalize`] turns a platform key name + modifiers into a neutral
//! [`KeyEvent`] with a canonical [`KeyCode`] (letters/digits normalized),
//! so Windows / macOS / Linux / Android / iOS share one key semantic.
//! [`PlatformSemantics`] makes the *primary* shortcut modifier explicit
//! (Ctrl everywhere, Cmd on macOS). IME composition is first-class: while a
//! composition is active, shortcut chords are suppressed. [`KeyBindingConfig`]
//! maps chords to actions and is fully user-remappable.

/// The platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux.
    Linux,
    /// Android.
    Android,
    /// iOS.
    Ios,
}

/// A neutral key code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyCode {
    /// A normalized letter or digit (`'a'..='z'`, `'0'..='9'`).
    Char(char),
    /// A function key F1..=F12.
    F(u8),
    /// An arrow key.
    Arrow(Direction),
    /// Tab.
    Tab,
    /// Enter / Return.
    Enter,
    /// Escape.
    Escape,
    /// Backspace.
    Backspace,
    /// Space.
    Space,
    /// Delete.
    Delete,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
}

/// An arrow direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    /// Up.
    Up,
    /// Down.
    Down,
    /// Left.
    Left,
    /// Right.
    Right,
}

/// The modifier state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Ctrl.
    pub ctrl: bool,
    /// Alt / Option.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// Meta (Cmd on macOS, Windows/Search elsewhere).
    pub meta: bool,
}

impl Modifiers {
    /// A new modifier set.
    pub fn new(ctrl: bool, alt: bool, shift: bool, meta: bool) -> Self {
        Self {
            ctrl,
            alt,
            shift,
            meta,
        }
    }

    /// Whether any modifier is pressed.
    pub fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.meta
    }
}

/// A normalized keyboard event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    /// The neutral key code.
    pub code: KeyCode,
    /// The modifier state.
    pub modifiers: Modifiers,
    /// Text produced by the key (printable / IME composition result).
    pub text: Option<String>,
    /// Whether an IME composition is active (shortcuts suppressed).
    pub composing: bool,
}

/// The primary shortcut modifier for a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryModifier {
    /// Ctrl (Windows / Linux / Android / iOS).
    Ctrl,
    /// Cmd / Meta (macOS).
    Meta,
}

/// Per-platform shortcut semantics.
pub struct PlatformSemantics;

impl PlatformSemantics {
    /// The primary shortcut modifier for a platform.
    pub fn primary_modifier(platform: Platform) -> PrimaryModifier {
        match platform {
            Platform::MacOs => PrimaryModifier::Meta,
            _ => PrimaryModifier::Ctrl,
        }
    }

    /// The human label of a modifier on a platform (e.g. "Ctrl" vs "Cmd").
    pub fn modifier_label(platform: Platform, modifier: ModifierKey) -> &'static str {
        match (platform, modifier) {
            (Platform::MacOs, ModifierKey::Meta) => "Cmd",
            (Platform::MacOs, ModifierKey::Alt) => "Option",
            (_, ModifierKey::Meta) => "Win",
            (_, ModifierKey::Alt) => "Alt",
            (_, ModifierKey::Ctrl) => "Ctrl",
            (_, ModifierKey::Shift) => "Shift",
        }
    }
}

/// Parses a canonical key name into a neutral [`KeyCode`].
/// Accepts "a"/"A", "0".."9", "f1".."f12", "arrowup"/"up", "tab", "enter",
/// "escape"/"esc", "backspace", "space", "delete", "home", "end",
/// "pageup", "pagedown".
pub fn parse_key(name: &str) -> Option<KeyCode> {
    let normalized = name.to_ascii_lowercase();
    match normalized.as_str() {
        "tab" => return Some(KeyCode::Tab),
        "enter" | "return" => return Some(KeyCode::Enter),
        "escape" | "esc" => return Some(KeyCode::Escape),
        "backspace" => return Some(KeyCode::Backspace),
        "space" => return Some(KeyCode::Space),
        "delete" | "del" => return Some(KeyCode::Delete),
        "home" => return Some(KeyCode::Home),
        "end" => return Some(KeyCode::End),
        "pageup" | "pgup" => return Some(KeyCode::PageUp),
        "pagedown" | "pgdn" => return Some(KeyCode::PageDown),
        "arrowup" | "up" => return Some(KeyCode::Arrow(Direction::Up)),
        "arrowdown" | "down" => return Some(KeyCode::Arrow(Direction::Down)),
        "arrowleft" | "left" => return Some(KeyCode::Arrow(Direction::Left)),
        "arrowright" | "right" => return Some(KeyCode::Arrow(Direction::Right)),
        _ => {}
    }
    // Single letter or digit.
    if let Some(first) = normalized.chars().next() {
        if normalized.chars().count() == 1 {
            if first.is_ascii_lowercase() && first.is_ascii_alphabetic() {
                return Some(KeyCode::Char(first));
            }
            if first.is_ascii_digit() {
                return Some(KeyCode::Char(first));
            }
        }
    }
    // Function keys.
    if let Some(rest) = normalized.strip_prefix('f') {
        if let Ok(number) = rest.parse::<u8>() {
            if (1..=12).contains(&number) {
                return Some(KeyCode::F(number));
            }
        }
    }
    None
}

/// A key normalization map.
pub struct KeyMap;

impl KeyMap {
    /// Normalizes a canonical key name + modifiers into a neutral event.
    pub fn normalize(
        key_name: &str,
        modifiers: Modifiers,
        text: Option<String>,
        composing: bool,
    ) -> Option<KeyEvent> {
        Some(KeyEvent {
            code: parse_key(key_name)?,
            modifiers,
            text,
            composing,
        })
    }
}

/// A shortcut modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    /// Ctrl.
    Ctrl,
    /// Alt / Option.
    Alt,
    /// Shift.
    Shift,
    /// Meta (Cmd / Win / Search).
    Meta,
}

/// A user action bound to a keyboard chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyAction {
    /// Copy.
    Copy,
    /// Paste.
    Paste,
    /// New tab.
    NewTab,
    /// Close tab.
    CloseTab,
    /// Next tab.
    NextTab,
    /// Find.
    Find,
}

/// A keyboard chord: modifier keys + a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    /// Required modifier keys.
    pub modifiers: Vec<ModifierKey>,
    /// The key.
    pub key: KeyCode,
}

/// The configurable shortcut map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindingConfig {
    bindings: Vec<(KeyAction, Chord)>,
}

impl KeyBindingConfig {
    /// The default bindings (primary modifier = Ctrl, or Cmd on macOS).
    pub fn defaults(platform: Platform) -> Self {
        let primary = match PlatformSemantics::primary_modifier(platform) {
            PrimaryModifier::Ctrl => ModifierKey::Ctrl,
            PrimaryModifier::Meta => ModifierKey::Meta,
        };
        let mut config = Self {
            bindings: Vec::new(),
        };
        config.bind(KeyAction::Copy, primary, KeyCode::Char('c'));
        config.bind(KeyAction::Paste, primary, KeyCode::Char('v'));
        config.bind(KeyAction::NewTab, primary, KeyCode::Char('t'));
        config.bind(KeyAction::CloseTab, primary, KeyCode::Char('w'));
        config.bind(KeyAction::NextTab, primary, KeyCode::Tab);
        config.bind(KeyAction::Find, primary, KeyCode::Char('f'));
        config
    }

    fn bind(&mut self, action: KeyAction, modifier: ModifierKey, key: KeyCode) {
        self.bindings.push((
            action,
            Chord {
                modifiers: vec![modifier],
                key,
            },
        ));
    }

    /// Remaps an action to a new chord.
    pub fn set_binding(&mut self, action: KeyAction, chord: Chord) {
        self.bindings.retain(|(existing, _)| *existing != action);
        self.bindings.push((action, chord));
    }

    /// Removes a binding; returns the previous chord.
    pub fn clear_binding(&mut self, action: KeyAction) -> Option<Chord> {
        let index = self.bindings.iter().position(|(a, _)| *a == action)?;
        Some(self.bindings.remove(index).1)
    }

    /// Resolves an event to an action. `None` during IME composition, for
    /// keys with text, or when no chord matches.
    pub fn resolve(&self, event: &KeyEvent) -> Option<KeyAction> {
        if event.composing || event.text.is_some() {
            return None;
        }
        for (action, chord) in &self.bindings {
            if chord.key != event.code {
                continue;
            }
            let mut matched = true;
            for modifier in &chord.modifiers {
                match *modifier {
                    ModifierKey::Ctrl if !event.modifiers.ctrl => matched = false,
                    ModifierKey::Alt if !event.modifiers.alt => matched = false,
                    ModifierKey::Shift if !event.modifiers.shift => matched = false,
                    ModifierKey::Meta if !event.modifiers.meta => matched = false,
                    _ => {}
                }
            }
            if matched {
                return Some(*action);
            }
        }
        None
    }

    /// The human chord label on a platform (e.g. "Ctrl+T" / "Cmd+T").
    pub fn chord_label(&self, action: KeyAction, platform: Platform) -> Option<String> {
        let (_, chord) = self.bindings.iter().find(|(a, _)| *a == action)?;
        let parts: Vec<&str> = chord
            .modifiers
            .iter()
            .map(|modifier| PlatformSemantics::modifier_label(platform, *modifier))
            .collect();
        let mut label = parts.join("+");
        if !label.is_empty() {
            label.push('+');
        }
        label.push_str(&key_label(chord.key));
        Some(label)
    }
}

/// A short human label for a key.
pub fn key_label(code: KeyCode) -> String {
    match code {
        KeyCode::Char(character) => character.to_ascii_uppercase().to_string(),
        KeyCode::F(number) => format!("F{number}"),
        KeyCode::Arrow(Direction::Up) => "↑".to_owned(),
        KeyCode::Arrow(Direction::Down) => "↓".to_owned(),
        KeyCode::Arrow(Direction::Left) => "←".to_owned(),
        KeyCode::Arrow(Direction::Right) => "→".to_owned(),
        KeyCode::Tab => "Tab".to_owned(),
        KeyCode::Enter => "Enter".to_owned(),
        KeyCode::Escape => "Esc".to_owned(),
        KeyCode::Backspace => "Backspace".to_owned(),
        KeyCode::Space => "Space".to_owned(),
        KeyCode::Delete => "Del".to_owned(),
        KeyCode::Home => "Home".to_owned(),
        KeyCode::End => "End".to_owned(),
        KeyCode::PageUp => "PgUp".to_owned(),
        KeyCode::PageDown => "PgDn".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        key_label, parse_key, Chord, Direction, KeyAction, KeyBindingConfig, KeyCode, KeyMap,
        ModifierKey, Modifiers, Platform, PlatformSemantics, PrimaryModifier,
    };

    #[test]
    fn key_codes_parse_and_normalize() {
        assert_eq!(parse_key("a"), Some(KeyCode::Char('a')));
        assert_eq!(
            parse_key("A"),
            Some(KeyCode::Char('a')),
            "letters normalize"
        );
        assert_eq!(parse_key("5"), Some(KeyCode::Char('5')));
        assert_eq!(parse_key("f10"), Some(KeyCode::F(10)));
        assert_eq!(parse_key("f13"), None, "only F1..F12");
        assert_eq!(
            parse_key("arrowleft"),
            Some(KeyCode::Arrow(Direction::Left))
        );
        assert_eq!(parse_key("esc"), Some(KeyCode::Escape));
        assert_eq!(parse_key("nope"), None);
    }

    #[test]
    fn platform_primary_modifier_semantics() {
        assert_eq!(
            PlatformSemantics::primary_modifier(Platform::Windows),
            PrimaryModifier::Ctrl
        );
        assert_eq!(
            PlatformSemantics::primary_modifier(Platform::Linux),
            PrimaryModifier::Ctrl
        );
        assert_eq!(
            PlatformSemantics::primary_modifier(Platform::Android),
            PrimaryModifier::Ctrl
        );
        assert_eq!(
            PlatformSemantics::primary_modifier(Platform::Ios),
            PrimaryModifier::Ctrl
        );
        assert_eq!(
            PlatformSemantics::primary_modifier(Platform::MacOs),
            PrimaryModifier::Meta
        );
        assert_eq!(
            PlatformSemantics::modifier_label(Platform::MacOs, ModifierKey::Meta),
            "Cmd"
        );
        assert_eq!(
            PlatformSemantics::modifier_label(Platform::Windows, ModifierKey::Meta),
            "Win"
        );
    }

    #[test]
    fn keyboard_event_matrix_is_platform_consistent() {
        // The same chord resolves identically on every platform: Ctrl+T on
        // Windows/Linux/Android/iOS, Cmd+T on macOS.
        for platform in [
            Platform::Windows,
            Platform::Linux,
            Platform::Android,
            Platform::Ios,
            Platform::MacOs,
        ] {
            let config = KeyBindingConfig::defaults(platform);
            let primary = PlatformSemantics::primary_modifier(platform);
            let event = match primary {
                PrimaryModifier::Ctrl => {
                    KeyMap::normalize("t", Modifiers::new(true, false, false, false), None, false)
                        .unwrap()
                }
                PrimaryModifier::Meta => {
                    KeyMap::normalize("t", Modifiers::new(false, false, false, true), None, false)
                        .unwrap()
                }
            };
            assert_eq!(
                config.resolve(&event),
                Some(KeyAction::NewTab),
                "new-tab chord must be consistent on {platform:?}"
            );
            // Labels differ per platform but stay readable.
            assert_eq!(
                config.chord_label(KeyAction::NewTab, platform).as_deref(),
                Some(if platform == Platform::MacOs {
                    "Cmd+T"
                } else {
                    "Ctrl+T"
                })
            );
        }
    }

    #[test]
    fn ime_composition_suppresses_shortcuts() {
        let config = KeyBindingConfig::defaults(Platform::Windows);
        // Composing with text: the chord is NOT a shortcut (IME owns it).
        let composing = KeyMap::normalize(
            "t",
            Modifiers::new(true, false, false, false),
            Some("t".to_owned()),
            true,
        )
        .unwrap();
        assert_eq!(config.resolve(&composing), None);
        // A plain printable key without composition is also not a shortcut.
        let plain = KeyMap::normalize(
            "t",
            Modifiers::new(true, false, false, false),
            Some("t".to_owned()),
            false,
        )
        .unwrap();
        assert_eq!(config.resolve(&plain), None);
        // Without text / composition the chord resolves.
        let chord =
            KeyMap::normalize("t", Modifiers::new(true, false, false, false), None, false).unwrap();
        assert_eq!(config.resolve(&chord), Some(KeyAction::NewTab));
    }

    #[test]
    fn shortcuts_are_configurable_and_remappable() {
        let mut config = KeyBindingConfig::defaults(Platform::Windows);
        // Remap NewTab from Ctrl+T to Ctrl+N.
        config.set_binding(
            KeyAction::NewTab,
            Chord {
                modifiers: vec![ModifierKey::Ctrl],
                key: KeyCode::Char('n'),
            },
        );
        let old =
            KeyMap::normalize("t", Modifiers::new(true, false, false, false), None, false).unwrap();
        assert_ne!(config.resolve(&old), Some(KeyAction::NewTab));
        let new =
            KeyMap::normalize("n", Modifiers::new(true, false, false, false), None, false).unwrap();
        assert_eq!(config.resolve(&new), Some(KeyAction::NewTab));
        assert_eq!(
            config
                .chord_label(KeyAction::NewTab, Platform::Windows)
                .as_deref(),
            Some("Ctrl+N")
        );
        // Clearing a binding removes it.
        assert!(config.clear_binding(KeyAction::NewTab).is_some());
        assert!(config.resolve(&new).is_none());
    }

    #[test]
    fn key_labels_are_readable() {
        assert_eq!(key_label(KeyCode::Char('t')), "T");
        assert_eq!(key_label(KeyCode::F(5)), "F5");
        assert_eq!(key_label(KeyCode::Arrow(Direction::Up)), "↑");
        assert_eq!(key_label(KeyCode::Escape), "Esc");
    }
}
