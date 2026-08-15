//! Mobile session stack, bottom action bar, and safe-area adaptation (T107).
//!
//! The mobile layout model derives a [`FormFactor`] (phone/tablet) and
//! [`Orientation`] (portrait/landscape) from the viewport, applies system
//! safe-area insets (notch/gesture bars), lays out the bottom action bar
//! deterministically per form factor (golden-testable), and provides a
//! [`SessionStack`] whose [`SessionStack::on_system_back`] implements the
//! Android/iOS system-back contract (pop when there is history, exit
//! otherwise).

/// The device form factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFactor {
    /// Phone-sized viewport.
    Phone,
    /// Tablet-sized viewport.
    Tablet,
}

/// The viewport orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Taller than wide.
    Portrait,
    /// Wider than tall.
    Landscape,
}

/// A viewport size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// Width in logical pixels.
    pub width: u32,
    /// Height in logical pixels.
    pub height: u32,
}

impl Viewport {
    /// The form factor: a tablet when the smaller dimension is >= 600
    /// (a phone in landscape keeps its phone form factor).
    pub fn form_factor(&self) -> FormFactor {
        if self.width.min(self.height) >= 600 {
            FormFactor::Tablet
        } else {
            FormFactor::Phone
        }
    }

    /// The orientation.
    pub fn orientation(&self) -> Orientation {
        if self.height >= self.width {
            Orientation::Portrait
        } else {
            Orientation::Landscape
        }
    }

    /// One-handed compatibility: a phone in portrait is reachable with one
    /// thumb; wider/landscape layouts are not.
    pub fn is_one_handed_compatible(&self) -> bool {
        self.form_factor() == FormFactor::Phone && self.orientation() == Orientation::Portrait
    }
}

/// System safe-area insets (status bar, gesture bar, notches).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafeAreaInsets {
    /// Top inset.
    pub top: u32,
    /// Bottom inset.
    pub bottom: u32,
    /// Left inset.
    pub left: u32,
    /// Right inset.
    pub right: u32,
}

/// Computes the effective safe area for a viewport from the raw system
/// insets. Landscape phones gain side insets (for a display cutout); tablets
/// keep the system values.
pub fn effective_safe_area(viewport: &Viewport, system: SafeAreaInsets) -> SafeAreaInsets {
    match viewport.form_factor() {
        FormFactor::Phone if viewport.orientation() == Orientation::Landscape => SafeAreaInsets {
            top: system.top,
            bottom: system.bottom,
            left: system.left.max(16),
            right: system.right.max(16),
        },
        _ => system,
    }
}

/// The bottom action bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BottomActionBar {
    /// Whether the bar is shown.
    pub visible: bool,
    /// Whether the bar is expanded (tablet / landscape shows more actions).
    pub expanded: bool,
}

impl BottomActionBar {
    /// The default bar: visible, collapsed.
    pub fn new() -> Self {
        Self {
            visible: true,
            expanded: false,
        }
    }

    /// Adapts the bar to a viewport (golden-testable metrics).
    pub fn layout(&self, viewport: &Viewport, safe: SafeAreaInsets) -> BarLayout {
        let expanded = self.expanded
            || viewport.form_factor() == FormFactor::Tablet
            || viewport.orientation() == Orientation::Landscape;
        let item_count = if expanded { 5 } else { 3 };
        let height = if expanded { 56 } else { 48 };
        let width = viewport.width;
        let y = viewport.height.saturating_sub(height + safe.bottom) as i32;
        let x = safe.left as i32;
        BarLayout {
            x,
            y,
            width: width.saturating_sub(safe.left + safe.right),
            height,
            item_count,
        }
    }
}

impl Default for BottomActionBar {
    fn default() -> Self {
        Self::new()
    }
}

/// The deterministic bar layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarLayout {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width (inside safe area).
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Number of actions shown.
    pub item_count: usize,
}

/// The system-back result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemBack {
    /// The back was consumed (popped a session).
    Consumed,
    /// No history left; the app should exit.
    ShouldExit,
}

/// The mobile session navigation stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStack {
    stack: Vec<u64>,
}

impl SessionStack {
    /// A stack rooted at a session.
    pub fn new(root_session: u64) -> Self {
        Self {
            stack: vec![root_session],
        }
    }

    /// The current session id.
    pub fn current(&self) -> u64 {
        *self.stack.last().expect("stack never empty")
    }

    /// The navigation depth.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Pushes a session onto the stack.
    pub fn push(&mut self, session: u64) {
        self.stack.push(session);
    }

    /// Handles the system back: pops when there is history, otherwise asks
    /// the app to exit.
    pub fn on_system_back(&mut self) -> SystemBack {
        if self.stack.len() > 1 {
            self.stack.pop();
            SystemBack::Consumed
        } else {
            SystemBack::ShouldExit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        effective_safe_area, BarLayout, BottomActionBar, FormFactor, Orientation, SafeAreaInsets,
        SessionStack, SystemBack, Viewport,
    };

    fn safe(top: u32, bottom: u32, left: u32, right: u32) -> SafeAreaInsets {
        SafeAreaInsets {
            top,
            bottom,
            left,
            right,
        }
    }

    #[test]
    fn form_factor_and_orientation_detection() {
        let phone = Viewport {
            width: 390,
            height: 844,
        };
        assert_eq!(phone.form_factor(), FormFactor::Phone);
        assert_eq!(phone.orientation(), Orientation::Portrait);
        assert!(phone.is_one_handed_compatible());

        let phone_landscape = Viewport {
            width: 844,
            height: 390,
        };
        assert_eq!(phone_landscape.form_factor(), FormFactor::Phone);
        assert_eq!(phone_landscape.orientation(), Orientation::Landscape);
        assert!(!phone_landscape.is_one_handed_compatible());

        let tablet = Viewport {
            width: 1024,
            height: 1366,
        };
        assert_eq!(tablet.form_factor(), FormFactor::Tablet);
        assert!(!tablet.is_one_handed_compatible());
    }

    #[test]
    fn safe_area_adapts_to_landscape_phone_cutout() {
        let phone_landscape = Viewport {
            width: 844,
            height: 390,
        };
        let effective = effective_safe_area(&phone_landscape, safe(0, 24, 0, 0));
        assert_eq!(effective.left, 16, "landscape phone gains a side inset");
        assert_eq!(effective.right, 16);
        // Tablet keeps system values.
        let tablet = Viewport {
            width: 1024,
            height: 1366,
        };
        let effective = effective_safe_area(&tablet, safe(24, 24, 0, 0));
        assert_eq!(effective, safe(24, 24, 0, 0));
    }

    #[test]
    fn bottom_bar_layout_golden_phone_tablet_landscape() {
        let bar = BottomActionBar::new();
        // Phone portrait: collapsed 3 actions, 48px tall.
        let phone = bar.layout(
            &Viewport {
                width: 390,
                height: 844,
            },
            safe(47, 34, 0, 0),
        );
        assert_eq!(
            phone,
            BarLayout {
                x: 0,
                y: 844 - 48 - 34,
                width: 390,
                height: 48,
                item_count: 3
            }
        );
        // Tablet: expanded 5 actions, 56px tall.
        let tablet = bar.layout(
            &Viewport {
                width: 1024,
                height: 1366,
            },
            safe(24, 24, 0, 0),
        );
        assert_eq!(
            tablet,
            BarLayout {
                x: 0,
                y: 1366 - 56 - 24,
                width: 1024,
                height: 56,
                item_count: 5
            }
        );
        // Phone landscape: expanded, side insets applied to width/x.
        let landscape = bar.layout(
            &Viewport {
                width: 844,
                height: 390,
            },
            safe(0, 24, 16, 16),
        );
        assert_eq!(
            landscape,
            BarLayout {
                x: 16,
                y: 390 - 56 - 24,
                width: 844 - 32,
                height: 56,
                item_count: 5
            }
        );
    }

    #[test]
    fn session_stack_system_back_pops_then_exits() {
        let mut stack = SessionStack::new(1);
        stack.push(2);
        stack.push(3);
        assert_eq!(stack.current(), 3);
        assert_eq!(stack.depth(), 3);
        assert_eq!(stack.on_system_back(), SystemBack::Consumed);
        assert_eq!(stack.current(), 2);
        assert_eq!(stack.on_system_back(), SystemBack::Consumed);
        assert_eq!(stack.current(), 1);
        assert_eq!(stack.on_system_back(), SystemBack::ShouldExit);
        assert_eq!(stack.current(), 1, "root stays after should-exit");
    }
}
