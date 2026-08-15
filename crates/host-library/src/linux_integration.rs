//! Linux Wayland / X11, Secret Service, notification, and desktop-entry
//! model (T125).
//!
//! Models the Linux contract: display server (X11 / Wayland), desktop
//! environment (GNOME / KDE / other), scaling, clipboard behavior per display
//! server, Secret Service availability with a no-keyring fallback policy, a
//! valid desktop entry, and secret-free notifications. The real
//! distro / display-server matrix runs on Linux hosts.

/// The display server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    /// X11.
    X11,
    /// Wayland.
    Wayland,
}

impl DisplayServer {
    /// Whether the legacy X11 clipboard can be read without a clipboard
    /// manager (X11 provides selection; Wayland requires a manager).
    pub fn has_legacy_clipboard(&self) -> bool {
        matches!(self, DisplayServer::X11)
    }
}

/// The desktop environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopEnvironment {
    /// GNOME.
    Gnome,
    /// KDE Plasma.
    Kde,
    /// Other.
    Other,
}

/// Fractional scaling support per display server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScalingPolicy {
    /// The display server.
    pub display: DisplayServer,
    /// Whether fractional scaling is supported.
    pub fractional_scaling: bool,
}

impl ScalingPolicy {
    /// The scaling policy for a display server (X11 lacks native fractional
    /// scaling; Wayland supports it).
    pub fn for_display(display: DisplayServer) -> Self {
        Self {
            display,
            fractional_scaling: matches!(display, DisplayServer::Wayland),
        }
    }
}

/// The no-keyring fallback behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoKeyringPolicy {
    /// Refuse to store secrets without a Secret Service.
    Refuse,
    /// Keep secrets in memory only (never persisted).
    MemoryOnly,
}

/// Secret Service availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretServiceState {
    /// Whether a Secret Service (gnome-keyring / KWallet) is available.
    pub available: bool,
    /// The no-keyring fallback policy.
    pub fallback: NoKeyringPolicy,
}

impl SecretServiceState {
    /// Detects the state: available with gnome-keyring/KWallet, or a
    /// no-keyring fallback.
    pub fn detect(available: bool, environment: DesktopEnvironment) -> Self {
        let fallback = if available {
            NoKeyringPolicy::Refuse
        } else if environment == DesktopEnvironment::Other {
            // Headless / other: never store secrets to disk.
            NoKeyringPolicy::Refuse
        } else {
            NoKeyringPolicy::MemoryOnly
        };
        Self {
            available,
            fallback,
        }
    }

    /// Whether secrets may be persisted.
    pub fn can_persist(&self) -> bool {
        self.available
    }
}

/// A desktop entry (.desktop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// Application name.
    pub name: String,
    /// Executable.
    pub exec: String,
    /// Icon name.
    pub icon: String,
    /// Categories.
    pub categories: String,
    /// Startup notification.
    pub startup_notify: bool,
}

impl DesktopEntry {
    /// The minimal desktop entry.
    pub fn minimal() -> Self {
        Self {
            name: "SSH Desktop".to_owned(),
            exec: "ssh-desktop".to_owned(),
            icon: "utilities-terminal".to_owned(),
            categories: "Network;RemoteAccess;".to_owned(),
            startup_notify: true,
        }
    }

    /// A valid `.desktop` file body.
    pub fn to_desktop_file(&self) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon={}\nCategories={}\nStartupNotify={}\n",
            self.name,
            self.exec,
            self.icon,
            self.categories,
            if self.startup_notify { "true" } else { "false" }
        )
    }
}

/// A secret-free Linux notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxNotification {
    /// Title.
    pub title: String,
    /// Body.
    pub body: String,
}

impl LinuxNotification {
    /// Whether the notification contains `needle` (leak check).
    pub fn contains(&self, needle: &str) -> bool {
        self.title.contains(needle) || self.body.contains(needle)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DesktopEntry, DesktopEnvironment, DisplayServer, LinuxNotification, NoKeyringPolicy,
        ScalingPolicy, SecretServiceState,
    };

    #[test]
    fn display_server_clipboard_and_scaling() {
        assert!(DisplayServer::X11.has_legacy_clipboard());
        assert!(!DisplayServer::Wayland.has_legacy_clipboard());
        assert!(!ScalingPolicy::for_display(DisplayServer::X11).fractional_scaling);
        assert!(ScalingPolicy::for_display(DisplayServer::Wayland).fractional_scaling);
    }

    #[test]
    fn secret_service_and_no_keyring_fallback() {
        let available = SecretServiceState::detect(true, DesktopEnvironment::Gnome);
        assert!(available.can_persist());
        let gnome_none = SecretServiceState::detect(false, DesktopEnvironment::Gnome);
        assert!(!gnome_none.can_persist());
        assert_eq!(gnome_none.fallback, NoKeyringPolicy::MemoryOnly);
        let headless = SecretServiceState::detect(false, DesktopEnvironment::Other);
        assert_eq!(headless.fallback, NoKeyringPolicy::Refuse);
    }

    #[test]
    fn desktop_entry_generates_valid_file() {
        let entry = DesktopEntry::minimal();
        let file = entry.to_desktop_file();
        assert!(file.starts_with("[Desktop Entry]"));
        assert!(file.contains("Type=Application"));
        assert!(file.contains("Name=SSH Desktop"));
        assert!(file.contains("Exec=ssh-desktop"));
        assert!(file.contains("Categories=Network;RemoteAccess;"));
        assert!(file.contains("StartupNotify=true"));
    }

    #[test]
    fn notifications_do_not_leak_secrets() {
        let notification = LinuxNotification {
            title: "Update available".to_owned(),
            body: "Version 1.2 is ready.".to_owned(),
        };
        assert!(!notification.contains("KEY_MATERIAL"));
        assert!(notification.contains("1.2"));
    }
}
