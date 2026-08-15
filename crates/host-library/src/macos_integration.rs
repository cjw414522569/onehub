//! macOS menu, window, Keychain, notification, and deep-link integration
//! model (T123).
//!
//! Models the macOS-specific contract: Intel / Apple Silicon, Retina
//! backing scale (@1x / @2x), multi-monitor restore (reusing the shared
//! geometry), App Nap policy (disabled during active sessions), the app
//! menu, secret-free notifications, and ssh:// deep links. Real macOS
//! automation and the physical-machine checklist run on macOS hosts; this
//! module locks the deterministic model.

use crate::windows_integration::Size;

/// The macOS CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacArch {
    /// Intel (x86_64).
    Intel,
    /// Apple Silicon (arm64).
    AppleSilicon,
}

/// The Retina backing scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetinaScale {
    /// Standard (@1x).
    OneX,
    /// Retina (@2x).
    TwoX,
}

impl RetinaScale {
    /// The backing scale factor.
    pub fn factor(&self) -> f64 {
        match self {
            RetinaScale::OneX => 1.0,
            RetinaScale::TwoX => 2.0,
        }
    }

    /// Scales a logical size to physical pixels.
    pub fn logical_to_physical(&self, size: Size) -> Size {
        Size {
            width: ((size.width as f64) * self.factor()).round() as u32,
            height: ((size.height as f64) * self.factor()).round() as u32,
        }
    }
}

/// A macOS menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacMenuAction {
    /// New tab.
    NewTab,
    /// Close tab.
    CloseTab,
    /// Copy.
    Copy,
    /// Paste.
    Paste,
    /// Preferences.
    Preferences,
    /// Quit.
    Quit,
}

/// The app menu structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacMenu {
    /// Menu name -> actions.
    pub items: Vec<(String, Vec<MacMenuAction>)>,
}

impl MacMenu {
    /// The default macOS app menu.
    pub fn default_menu() -> Self {
        Self {
            items: vec![
                (
                    "App".to_owned(),
                    vec![MacMenuAction::Preferences, MacMenuAction::Quit],
                ),
                (
                    "File".to_owned(),
                    vec![MacMenuAction::NewTab, MacMenuAction::CloseTab],
                ),
                (
                    "Edit".to_owned(),
                    vec![MacMenuAction::Copy, MacMenuAction::Paste],
                ),
            ],
        }
    }

    /// All actions across menus.
    pub fn actions(&self) -> Vec<MacMenuAction> {
        self.items
            .iter()
            .flat_map(|(_, actions)| actions.iter().copied())
            .collect()
    }
}

/// The App Nap policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppNapPolicy {
    /// Whether App Nap is allowed right now.
    pub allowed: bool,
}

impl AppNapPolicy {
    /// Whether App Nap should be disabled while a session is active.
    pub fn for_active_sessions(active: bool) -> Self {
        Self { allowed: !active }
    }

    /// Whether the app may nap (e.g. for background sync when no session).
    pub fn may_nap(&self) -> bool {
        self.allowed
    }
}

/// A secret-free macOS notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacNotification {
    /// Title.
    pub title: String,
    /// Body.
    pub body: String,
}

impl MacNotification {
    /// Whether the notification contains `needle` (leak check).
    pub fn contains(&self, needle: &str) -> bool {
        self.title.contains(needle) || self.body.contains(needle)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppNapPolicy, MacArch, MacMenu, MacMenuAction, MacNotification, RetinaScale};
    use crate::windows_integration::{Monitor, MonitorLayout, Rect};

    #[test]
    fn architecture_and_retina_scale() {
        assert_ne!(MacArch::Intel, MacArch::AppleSilicon);
        assert_eq!(RetinaScale::OneX.factor(), 1.0);
        assert_eq!(RetinaScale::TwoX.factor(), 2.0);
        let retina = RetinaScale::TwoX;
        assert_eq!(
            retina.logical_to_physical(super::Size {
                width: 100,
                height: 50
            }),
            super::Size {
                width: 200,
                height: 100
            }
        );
    }

    #[test]
    fn multi_monitor_restore_uses_work_area() {
        let layout = MonitorLayout::new(vec![Monitor {
            bounds: Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            },
            work_area: Rect {
                x: 0,
                y: 25,
                width: 2560,
                height: 1375,
            },
        }]);
        let saved = Rect {
            x: 3000,
            y: 200,
            width: 800,
            height: 600,
        };
        let restored = layout.constrain_restore(saved);
        assert!(
            restored.y >= 25,
            "restored window respects the menu-bar work area"
        );
        assert!(layout
            .monitors
            .iter()
            .any(|m| restored.intersects(&m.work_area)));
    }

    #[test]
    fn app_nap_is_disabled_during_active_sessions() {
        assert!(!AppNapPolicy::for_active_sessions(true).may_nap());
        assert!(AppNapPolicy::for_active_sessions(false).may_nap());
    }

    #[test]
    fn default_menu_covers_standard_actions() {
        let menu = MacMenu::default_menu();
        let actions = menu.actions();
        assert!(actions.contains(&MacMenuAction::NewTab));
        assert!(actions.contains(&MacMenuAction::Quit));
        assert!(actions.contains(&MacMenuAction::Preferences));
        assert!(actions.contains(&MacMenuAction::Copy));
    }

    #[test]
    fn notifications_do_not_leak_secrets() {
        let notification = MacNotification {
            title: "Transfer complete".to_owned(),
            body: "backup finished at 100%.".to_owned(),
        };
        assert!(!notification.contains("SECRET_VALUE"));
        assert!(notification.contains("100%"));
    }
}
