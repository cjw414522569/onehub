//! Settings, theme, font, terminal, and network-policy UI model (T116).
//!
//! [`Settings`] groups appearance (theme), font, terminal, and network
//! policy. Every item declares its [`EffectTiming`] (immediate / on-reconnect
//! / on-restart), so the UI can label exactly when a change applies.
//! [`Settings::snapshot`] / [`Settings::from_snapshot`] provide versioned
//! persistence, [`migrate_snapshot`] upgrades older snapshots, and
//! [`Settings::reset_to_defaults`] restores the defaults.

use std::collections::BTreeMap;

/// The settings snapshot version.
pub const SETTINGS_VERSION: u32 = 1;

/// When a setting takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectTiming {
    /// Applies immediately.
    Immediate,
    /// Applies on the next connection.
    OnReconnect,
    /// Applies after a restart.
    OnRestart,
}

impl EffectTiming {
    /// A human label.
    pub fn label(&self) -> &'static str {
        match self {
            EffectTiming::Immediate => "Applies immediately",
            EffectTiming::OnReconnect => "Applies on reconnect",
            EffectTiming::OnRestart => "Applies on restart",
        }
    }
}

/// The proxy mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    /// No proxy.
    None,
    /// Use the system proxy.
    System,
    /// Manual proxy.
    Manual,
}

/// Appearance settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearanceSettings {
    /// Theme name.
    pub theme: String,
    /// High-contrast theme.
    pub high_contrast: bool,
}

/// Font settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSettings {
    /// Font family.
    pub family: String,
    /// Font size (8..=72).
    pub size: u32,
}

/// Terminal settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSettings {
    /// Audible bell.
    pub bell: bool,
    /// Blinking cursor.
    pub cursor_blink: bool,
    /// Scrollback lines (0..=1_000_000).
    pub scrollback_lines: u32,
}

/// Network policy settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkPolicySettings {
    /// Proxy mode.
    pub proxy_mode: ProxyMode,
    /// Keepalive seconds (0..=86400).
    pub keepalive_secs: u32,
    /// Allow SSH agent forwarding.
    pub allow_agent: bool,
}

/// All settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Appearance.
    pub appearance: AppearanceSettings,
    /// Font.
    pub font: FontSettings,
    /// Terminal.
    pub terminal: TerminalSettings,
    /// Network policy.
    pub network: NetworkPolicySettings,
}

impl Settings {
    /// The default settings.
    pub fn defaults() -> Self {
        Self {
            appearance: AppearanceSettings {
                theme: "dark".to_owned(),
                high_contrast: false,
            },
            font: FontSettings {
                family: "Cascadia Mono".to_owned(),
                size: 14,
            },
            terminal: TerminalSettings {
                bell: true,
                cursor_blink: true,
                scrollback_lines: 10_000,
            },
            network: NetworkPolicySettings {
                proxy_mode: ProxyMode::System,
                keepalive_secs: 30,
                allow_agent: false,
            },
        }
    }

    /// The effect timing for every setting key (for the UI labels).
    pub fn effect_timing(&self) -> Vec<(&'static str, EffectTiming)> {
        vec![
            ("appearance.theme", EffectTiming::Immediate),
            ("appearance.high_contrast", EffectTiming::Immediate),
            ("font.family", EffectTiming::Immediate),
            ("font.size", EffectTiming::Immediate),
            ("terminal.bell", EffectTiming::Immediate),
            ("terminal.cursor_blink", EffectTiming::Immediate),
            ("terminal.scrollback_lines", EffectTiming::OnReconnect),
            ("network.proxy_mode", EffectTiming::OnReconnect),
            ("network.keepalive_secs", EffectTiming::OnReconnect),
            ("network.allow_agent", EffectTiming::OnReconnect),
        ]
    }

    /// Restores every setting to its default.
    pub fn reset_to_defaults(&mut self) {
        *self = Self::defaults();
    }

    /// Serializes the settings to a versioned snapshot.
    pub fn snapshot(&self) -> SettingsSnapshot {
        let mut values = BTreeMap::new();
        values.insert("appearance.theme".to_owned(), self.appearance.theme.clone());
        values.insert(
            "appearance.high_contrast".to_owned(),
            self.appearance.high_contrast.to_string(),
        );
        values.insert("font.family".to_owned(), self.font.family.clone());
        values.insert("font.size".to_owned(), self.font.size.to_string());
        values.insert("terminal.bell".to_owned(), self.terminal.bell.to_string());
        values.insert(
            "terminal.cursor_blink".to_owned(),
            self.terminal.cursor_blink.to_string(),
        );
        values.insert(
            "terminal.scrollback_lines".to_owned(),
            self.terminal.scrollback_lines.to_string(),
        );
        values.insert(
            "network.proxy_mode".to_owned(),
            format!("{:?}", self.network.proxy_mode).to_lowercase(),
        );
        values.insert(
            "network.keepalive_secs".to_owned(),
            self.network.keepalive_secs.to_string(),
        );
        values.insert(
            "network.allow_agent".to_owned(),
            self.network.allow_agent.to_string(),
        );
        SettingsSnapshot {
            version: SETTINGS_VERSION,
            values,
        }
    }

    /// Restores settings from a snapshot, validating every known value.
    pub fn from_snapshot(snapshot: &SettingsSnapshot) -> Result<Self, SettingsError> {
        let mut settings = Self::defaults();
        for (key, value) in &snapshot.values {
            match key.as_str() {
                "appearance.theme" => settings.appearance.theme = value.clone(),
                "appearance.high_contrast" => {
                    settings.appearance.high_contrast = parse_bool(key, value)?
                }
                "font.family" => settings.font.family = value.clone(),
                "font.size" => {
                    let size = parse_number::<u32>(key, value)?;
                    if !(8..=72).contains(&size) {
                        return Err(SettingsError::InvalidValue(key.clone()));
                    }
                    settings.font.size = size;
                }
                "terminal.bell" => settings.terminal.bell = parse_bool(key, value)?,
                "terminal.cursor_blink" => settings.terminal.cursor_blink = parse_bool(key, value)?,
                "terminal.scrollback_lines" => {
                    let lines = parse_number::<u32>(key, value)?;
                    if lines > 1_000_000 {
                        return Err(SettingsError::InvalidValue(key.clone()));
                    }
                    settings.terminal.scrollback_lines = lines;
                }
                "network.proxy_mode" => {
                    settings.network.proxy_mode = match value.to_lowercase().as_str() {
                        "none" => ProxyMode::None,
                        "system" => ProxyMode::System,
                        "manual" => ProxyMode::Manual,
                        _ => return Err(SettingsError::InvalidValue(key.clone())),
                    };
                }
                "network.keepalive_secs" => {
                    let seconds = parse_number::<u32>(key, value)?;
                    if seconds > 86_400 {
                        return Err(SettingsError::InvalidValue(key.clone()));
                    }
                    settings.network.keepalive_secs = seconds;
                }
                "network.allow_agent" => settings.network.allow_agent = parse_bool(key, value)?,
                // Unknown keys are ignored for forward compatibility.
                _ => {}
            }
        }
        Ok(settings)
    }
}

/// Why loading settings failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// A known key has an unparsable or out-of-range value.
    InvalidValue(String),
}

/// A versioned settings snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSnapshot {
    /// The snapshot version.
    pub version: u32,
    /// Key -> string value.
    pub values: BTreeMap<String, String>,
}

/// Upgrades an older snapshot to the current version, filling any missing
/// keys with defaults (e.g. v0 -> v1 adds network.keepalive_secs).
pub fn migrate_snapshot(snapshot: &SettingsSnapshot) -> SettingsSnapshot {
    let mut values = snapshot.values.clone();
    let defaults = Settings::defaults().snapshot();
    for (key, default) in &defaults.values {
        values.entry(key.clone()).or_insert_with(|| default.clone());
    }
    SettingsSnapshot {
        version: SETTINGS_VERSION,
        values,
    }
}

fn parse_bool(key: &str, value: &str) -> Result<bool, SettingsError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(SettingsError::InvalidValue(key.to_owned())),
    }
}

fn parse_number<T: std::str::FromStr>(key: &str, value: &str) -> Result<T, SettingsError> {
    value
        .parse::<T>()
        .map_err(|_| SettingsError::InvalidValue(key.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{migrate_snapshot, EffectTiming, ProxyMode, Settings, SettingsSnapshot};

    #[test]
    fn defaults_restore_and_effect_timing_is_explicit() {
        let mut settings = Settings::defaults();
        settings.font.size = 20;
        settings.network.proxy_mode = ProxyMode::Manual;
        settings.reset_to_defaults();
        assert_eq!(settings, Settings::defaults());
        // Every item declares its effect timing explicitly.
        let timing = settings.effect_timing();
        assert_eq!(timing.len(), 10);
        assert!(timing
            .iter()
            .any(|(key, effect)| *key == "appearance.theme" && *effect == EffectTiming::Immediate));
        assert!(timing
            .iter()
            .any(|(key, effect)| *key == "network.proxy_mode"
                && *effect == EffectTiming::OnReconnect));
        assert!(timing.iter().all(|(_, effect)| effect.label().len() > 5));
    }

    #[test]
    fn persistence_round_trip() {
        let mut settings = Settings::defaults();
        settings.font.size = 16;
        settings.appearance.high_contrast = true;
        settings.network.keepalive_secs = 60;
        let snapshot = settings.snapshot();
        let restored = Settings::from_snapshot(&snapshot).unwrap();
        assert_eq!(restored, settings, "snapshot round-trips exactly");
        assert_eq!(snapshot.version, 1);
    }

    #[test]
    fn migration_fills_missing_keys_with_defaults() {
        // A v0 snapshot missing several keys (e.g. network.keepalive_secs).
        let mut values = std::collections::BTreeMap::new();
        values.insert("appearance.theme".to_owned(), "light".to_owned());
        values.insert("font.size".to_owned(), "18".to_owned());
        let v0 = SettingsSnapshot { version: 0, values };
        let migrated = migrate_snapshot(&v0);
        assert_eq!(migrated.version, 1);
        assert!(migrated.values.contains_key("network.keepalive_secs"));
        assert!(migrated.values.contains_key("terminal.scrollback_lines"));
        let restored = Settings::from_snapshot(&migrated).unwrap();
        assert_eq!(restored.appearance.theme, "light");
        assert_eq!(restored.font.size, 18);
        assert_eq!(restored.network.keepalive_secs, 30, "default fill");
    }

    #[test]
    fn invalid_values_are_rejected() {
        let mut snapshot = Settings::defaults().snapshot();
        snapshot
            .values
            .insert("font.size".to_owned(), "200".to_owned());
        assert!(Settings::from_snapshot(&snapshot).is_err());
        snapshot
            .values
            .insert("font.size".to_owned(), "not-a-number".to_owned());
        assert!(Settings::from_snapshot(&snapshot).is_err());
        snapshot
            .values
            .insert("network.proxy_mode".to_owned(), "nope".to_owned());
        assert!(Settings::from_snapshot(&snapshot).is_err());
        snapshot
            .values
            .insert("network.proxy_mode".to_owned(), "manual".to_owned());
        snapshot
            .values
            .insert("terminal.bell".to_owned(), "yes".to_owned());
        assert!(Settings::from_snapshot(&snapshot).is_err());
    }

    #[test]
    fn unknown_keys_are_forward_compatible() {
        let mut snapshot = Settings::defaults().snapshot();
        snapshot
            .values
            .insert("future.setting".to_owned(), "x".to_owned());
        let restored = Settings::from_snapshot(&snapshot).unwrap();
        assert_eq!(restored, Settings::defaults());
    }
}
