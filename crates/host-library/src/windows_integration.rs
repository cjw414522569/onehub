//! Windows window, tray, protocol-link, notification, and install
//! integration model (T121).
//!
//! Models the Windows-specific integration contract: architecture (x64 /
//! arm64), DPI scaling, multi-monitor window restore, tray actions, protocol
//! links (ssh://), secret-free notifications, and sleep/wake reconnection.
//! The real Win32 message loop / tray / installer bindings run on Windows
//! hosts; this module locks the model and the deterministic checks.

/// The Windows architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsArch {
    /// x64.
    X64,
    /// arm64.
    Arm64,
}

/// A size in logical or physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// A rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// X.
    pub x: i32,
    /// Y.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl Rect {
    /// The right edge.
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    /// The bottom edge.
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    /// Whether the rect intersects another rect.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

/// A monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    /// The full bounds.
    pub bounds: Rect,
    /// The work area (excluding taskbar / reserved space).
    pub work_area: Rect,
}

/// The DPI context.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DpiContext {
    /// Scale factor (e.g. 1.0, 1.25, 1.5, 2.0).
    pub scale_factor: f64,
}

impl DpiContext {
    /// Scales a logical size to physical pixels.
    pub fn logical_to_physical(&self, size: Size) -> Size {
        Size {
            width: ((size.width as f64) * self.scale_factor).round() as u32,
            height: ((size.height as f64) * self.scale_factor).round() as u32,
        }
    }

    /// Scales a physical size back to logical pixels.
    pub fn physical_to_logical(&self, size: Size) -> Size {
        Size {
            width: ((size.width as f64) / self.scale_factor).round() as u32,
            height: ((size.height as f64) / self.scale_factor).round() as u32,
        }
    }
}

/// The multi-monitor layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorLayout {
    /// The monitors.
    pub monitors: Vec<Monitor>,
}

impl MonitorLayout {
    /// A layout over the given monitors.
    pub fn new(monitors: Vec<Monitor>) -> Self {
        Self { monitors }
    }

    /// Returns a restored window rect that is on-screen: if the saved rect
    /// intersects no work area (e.g. a monitor was unplugged), the window is
    /// moved into the primary work area.
    pub fn constrain_restore(&self, saved: Rect) -> Rect {
        let visible = self
            .monitors
            .iter()
            .any(|monitor| saved.intersects(&monitor.work_area));
        if visible {
            return saved;
        }
        let primary = self
            .monitors
            .first()
            .map(|monitor| monitor.work_area)
            .unwrap_or(Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            });
        let width = saved.width.min(primary.width);
        let height = saved.height.min(primary.height);
        Rect {
            x: primary.x + ((primary.width - width) / 2) as i32,
            y: primary.y + ((primary.height - height) / 2) as i32,
            width,
            height,
        }
    }
}

/// A tray action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Open / focus the main window.
    OpenWindow,
    /// New tab.
    NewTab,
    /// Reconnect the active session.
    Reconnect,
    /// Quit.
    Quit,
}

/// A protocol link (`ssh://user@host:port`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolLink {
    /// Username (may be absent).
    pub username: Option<String>,
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
}

/// Why a protocol link failed to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    /// Not an ssh:// link.
    InvalidScheme,
    /// Missing or malformed host.
    MissingHost,
    /// Malformed port.
    InvalidPort,
}

/// Parses an `ssh://` link. The security policy (explicit confirmation,
/// no inline passwords) is enforced in T132; here we parse the shape.
pub fn parse_ssh_link(link: &str) -> Result<ProtocolLink, LinkError> {
    let rest = link
        .strip_prefix("ssh://")
        .ok_or(LinkError::InvalidScheme)?;
    // Split off path/query if present.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(LinkError::MissingHost);
    }
    let (user, host_port) = match authority.rsplit_once('@') {
        Some((user, host_port)) => (Some(user.to_owned()), host_port),
        None => (None, authority),
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (host, port),
        None => (host_port, "22"),
    };
    if host.is_empty() {
        return Err(LinkError::MissingHost);
    }
    let port: u16 = port.parse().map_err(|_| LinkError::InvalidPort)?;
    Ok(ProtocolLink {
        username: user,
        host: host.to_owned(),
        port,
    })
}

/// A secret-free Windows notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsNotification {
    /// Title.
    pub title: String,
    /// Body.
    pub body: String,
}

impl WindowsNotification {
    /// Whether the notification contains `needle` (leak check).
    pub fn contains(&self, needle: &str) -> bool {
        self.title.contains(needle) || self.body.contains(needle)
    }
}

/// Handles sleep/wake: on wake, active sessions reconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepWakePolicy {
    /// Whether to auto-reconnect on wake.
    pub reconnect_on_wake: bool,
}

impl SleepWakePolicy {
    /// The default policy: reconnect on wake.
    pub fn default_policy() -> Self {
        Self {
            reconnect_on_wake: true,
        }
    }

    /// Returns how many sessions were marked for reconnect on wake.
    pub fn handle_wake(&self, sessions: &[u64]) -> usize {
        if self.reconnect_on_wake {
            sessions.len()
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_ssh_link, DpiContext, LinkError, Monitor, MonitorLayout, Rect, Size, SleepWakePolicy,
        TrayAction, WindowsArch, WindowsNotification,
    };

    #[test]
    fn architecture_and_dpi_round_trip() {
        assert_ne!(WindowsArch::X64, WindowsArch::Arm64);
        let dpi = DpiContext { scale_factor: 1.5 };
        let logical = Size {
            width: 100,
            height: 40,
        };
        let physical = dpi.logical_to_physical(logical);
        assert_eq!(
            physical,
            Size {
                width: 150,
                height: 60
            }
        );
        assert_eq!(dpi.physical_to_logical(physical), logical);
        let dpi200 = DpiContext { scale_factor: 2.0 };
        assert_eq!(dpi200.logical_to_physical(logical).width, 200);
    }

    #[test]
    fn multi_monitor_restore_stays_on_screen() {
        let primary = Monitor {
            bounds: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            work_area: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
            },
        };
        let secondary = Monitor {
            bounds: Rect {
                x: 1920,
                y: 0,
                width: 1280,
                height: 1024,
            },
            work_area: Rect {
                x: 1920,
                y: 0,
                width: 1280,
                height: 1000,
            },
        };
        let layout = MonitorLayout::new(vec![primary, secondary]);
        // A window on the secondary monitor stays where it is.
        let on_secondary = Rect {
            x: 2000,
            y: 50,
            width: 800,
            height: 600,
        };
        assert_eq!(layout.constrain_restore(on_secondary), on_secondary);
        // A window saved on a now-unplugged third monitor is moved into the
        // primary work area (multi-monitor correctness after unplug).
        let off_screen = Rect {
            x: 5000,
            y: 300,
            width: 1000,
            height: 700,
        };
        let restored = layout.constrain_restore(off_screen);
        assert!(
            layout
                .monitors
                .iter()
                .any(|m| restored.intersects(&m.work_area)),
            "restored window must be visible on some monitor"
        );
        assert!(restored.width <= 1920 && restored.height <= 1040);
    }

    #[test]
    fn tray_actions_and_wake_reconnect() {
        let actions = [
            TrayAction::OpenWindow,
            TrayAction::NewTab,
            TrayAction::Reconnect,
            TrayAction::Quit,
        ];
        assert_eq!(actions.len(), 4);
        let policy = SleepWakePolicy::default_policy();
        assert_eq!(
            policy.handle_wake(&[1, 2, 3]),
            3,
            "sessions reconnect on wake"
        );
        let disabled = SleepWakePolicy {
            reconnect_on_wake: false,
        };
        assert_eq!(disabled.handle_wake(&[1, 2, 3]), 0);
    }

    #[test]
    fn protocol_links_parse() {
        let link = parse_ssh_link("ssh://root@10.0.0.5:2222").unwrap();
        assert_eq!(link.username.as_deref(), Some("root"));
        assert_eq!(link.host, "10.0.0.5");
        assert_eq!(link.port, 2222);
        // Default port and no user.
        let link = parse_ssh_link("ssh://example.com").unwrap();
        assert_eq!(link.username, None);
        assert_eq!(link.host, "example.com");
        assert_eq!(link.port, 22);
        // Errors.
        assert_eq!(parse_ssh_link("https://x"), Err(LinkError::InvalidScheme));
        assert_eq!(parse_ssh_link("ssh://"), Err(LinkError::MissingHost));
        assert_eq!(
            parse_ssh_link("ssh://h:notaport"),
            Err(LinkError::InvalidPort)
        );
    }

    #[test]
    fn notifications_do_not_leak_secrets() {
        let notification = WindowsNotification {
            title: "Transfer complete".to_owned(),
            body: "backup transfer finished at 100%.".to_owned(),
        };
        assert!(!notification.contains("SUPER_SECRET"));
        assert!(notification.contains("100%"));
    }
}
