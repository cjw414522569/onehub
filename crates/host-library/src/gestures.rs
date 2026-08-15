//! Mobile terminal extended keyboard and touch gestures (T109).
//!
//! The extended keyboard (Ctrl / Alt / Esc / Tab / arrow keys) is a separate
//! input surface from the touch canvas, so its keys never conflict with
//! scroll / selection gestures. The [`GestureRecognizer`] deterministically
//! disambiguates tap / long-press / scroll: a quick tap is a tap, holding
//! past the long-press threshold starts a selection, and dragging past the
//! scroll threshold scrolls. In selection mode a drag extends the selection
//! instead of scrolling.

/// A touch point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchPoint {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
}

impl TouchPoint {
    /// A point.
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A key on the mobile extended keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtendedKey {
    /// Ctrl.
    Ctrl,
    /// Alt.
    Alt,
    /// Escape.
    Esc,
    /// Tab.
    Tab,
    /// An arrow key.
    Arrow(super::keyboard::Direction),
}

/// A recognized touch gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// A quick tap at a point.
    Tap(TouchPoint),
    /// A long press started a selection at a point.
    SelectionStart(TouchPoint),
    /// A drag extended the selection to a point.
    SelectionExtend { to: TouchPoint },
    /// A one-finger pan: incremental scroll delta.
    Scroll { dx: i32, dy: i32 },
}

/// The current input mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal: drags scroll.
    Normal,
    /// Selection: drags extend the selection.
    Selection,
}

/// A chord emitted by the extended keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    /// Whether Ctrl is held.
    pub ctrl: bool,
    /// Whether Alt is held.
    pub alt: bool,
    /// The key.
    pub key: ExtendedKey,
}

/// The mobile extended keyboard (independent of the touch canvas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtendedKeyboard {
    ctrl: bool,
    alt: bool,
}

impl ExtendedKeyboard {
    /// An extended keyboard with no modifiers held.
    pub fn new() -> Self {
        Self {
            ctrl: false,
            alt: false,
        }
    }

    /// Presses a key: Ctrl/Alt set the held modifier; other keys emit a chord
    /// with the current modifier state.
    pub fn press(&mut self, key: ExtendedKey) -> Option<KeyChord> {
        match key {
            ExtendedKey::Ctrl => {
                self.ctrl = true;
                None
            }
            ExtendedKey::Alt => {
                self.alt = true;
                None
            }
            _ => Some(KeyChord {
                ctrl: self.ctrl,
                alt: self.alt,
                key,
            }),
        }
    }

    /// Releases a key (only modifiers matter).
    pub fn release(&mut self, key: ExtendedKey) {
        match key {
            ExtendedKey::Ctrl => self.ctrl = false,
            ExtendedKey::Alt => self.alt = false,
            _ => {}
        }
    }

    /// Whether Ctrl is held.
    pub fn ctrl_held(&self) -> bool {
        self.ctrl
    }

    /// Whether Alt is held.
    pub fn alt_held(&self) -> bool {
        self.alt
    }
}

impl Default for ExtendedKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

/// How long a press must last to become a long press.
pub const LONG_PRESS_MS: u64 = 500;
/// How far a drag must move to become a scroll / selection drag.
pub const SCROLL_THRESHOLD: i32 = 8;

#[derive(Debug, Clone, Copy)]
enum ActiveGesture {
    Tap { start: TouchPoint, down_ms: u64 },
    Scroll { last: TouchPoint },
    LongPressHeld,
}

/// A deterministic touch gesture recognizer.
#[derive(Debug, Clone)]
pub struct GestureRecognizer {
    mode: InputMode,
    active: Option<ActiveGesture>,
}

impl GestureRecognizer {
    /// A recognizer starting in normal mode.
    pub fn new() -> Self {
        Self {
            mode: InputMode::Normal,
            active: None,
        }
    }

    /// The current input mode.
    pub fn mode(&self) -> InputMode {
        self.mode
    }

    /// Switches the input mode (e.g. from a selection toolbar).
    pub fn set_mode(&mut self, mode: InputMode) {
        self.mode = mode;
    }

    /// A touch went down.
    pub fn on_touch_down(&mut self, point: TouchPoint, now_ms: u64) -> Option<Gesture> {
        self.active = Some(ActiveGesture::Tap {
            start: point,
            down_ms: now_ms,
        });
        None
    }

    /// A touch moved.
    pub fn on_touch_move(&mut self, point: TouchPoint, now_ms: u64) -> Option<Gesture> {
        match self.active {
            Some(ActiveGesture::Tap { start, down_ms }) => {
                let dx = point.x - start.x;
                let dy = point.y - start.y;
                if dx.abs() >= SCROLL_THRESHOLD || dy.abs() >= SCROLL_THRESHOLD {
                    self.active = Some(ActiveGesture::Scroll { last: point });
                    if self.mode == InputMode::Normal {
                        Some(Gesture::Scroll { dx, dy })
                    } else {
                        Some(Gesture::SelectionExtend { to: point })
                    }
                } else if now_ms.saturating_sub(down_ms) >= LONG_PRESS_MS {
                    self.active = Some(ActiveGesture::LongPressHeld);
                    if self.mode == InputMode::Normal {
                        self.mode = InputMode::Selection;
                        Some(Gesture::SelectionStart(start))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Some(ActiveGesture::Scroll { last }) => {
                let dx = point.x - last.x;
                let dy = point.y - last.y;
                self.active = Some(ActiveGesture::Scroll { last: point });
                if self.mode == InputMode::Normal {
                    Some(Gesture::Scroll { dx, dy })
                } else {
                    Some(Gesture::SelectionExtend { to: point })
                }
            }
            Some(ActiveGesture::LongPressHeld) => None,
            None => None,
        }
    }

    /// A touch went up.
    pub fn on_touch_up(&mut self, point: TouchPoint, now_ms: u64) -> Option<Gesture> {
        match self.active.take() {
            Some(ActiveGesture::Tap { start, down_ms }) => {
                let held = now_ms.saturating_sub(down_ms);
                let moved = (point.x - start.x).abs() >= SCROLL_THRESHOLD
                    || (point.y - start.y).abs() >= SCROLL_THRESHOLD;
                if moved {
                    None
                } else if held >= LONG_PRESS_MS {
                    if self.mode == InputMode::Normal {
                        self.mode = InputMode::Selection;
                        Some(Gesture::SelectionStart(start))
                    } else {
                        None
                    }
                } else if self.mode == InputMode::Selection {
                    // A tap in selection mode ends the selection.
                    self.mode = InputMode::Normal;
                    Some(Gesture::Tap(point))
                } else {
                    Some(Gesture::Tap(point))
                }
            }
            _ => None,
        }
    }
}

impl Default for GestureRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExtendedKey, ExtendedKeyboard, Gesture, GestureRecognizer, InputMode, KeyChord, TouchPoint,
        LONG_PRESS_MS, SCROLL_THRESHOLD,
    };
    use crate::keyboard::Direction;

    fn point(x: i32, y: i32) -> TouchPoint {
        TouchPoint::new(x, y)
    }

    #[test]
    fn tap_long_press_and_scroll_disambiguate() {
        let mut recognizer = GestureRecognizer::new();
        // Quick tap.
        recognizer.on_touch_down(point(10, 10), 0);
        assert_eq!(
            recognizer.on_touch_up(point(10, 10), 50),
            Some(Gesture::Tap(point(10, 10)))
        );
        assert_eq!(recognizer.mode(), InputMode::Normal);
        // Long press starts a selection.
        recognizer.on_touch_down(point(20, 20), 0);
        recognizer.on_touch_move(point(20, 20), LONG_PRESS_MS + 1);
        assert_eq!(
            recognizer.on_touch_up(point(20, 20), LONG_PRESS_MS + 2),
            None
        );
        assert_eq!(recognizer.mode(), InputMode::Selection);
        // Normal drag past the threshold scrolls.
        let mut recognizer = GestureRecognizer::new();
        recognizer.on_touch_down(point(0, 0), 0);
        let gesture = recognizer
            .on_touch_move(point(SCROLL_THRESHOLD + 4, 0), 100)
            .expect("scroll gesture");
        assert!(matches!(gesture, Gesture::Scroll { dx, dy } if dx > 0 && dy == 0));
        assert_eq!(recognizer.mode(), InputMode::Normal);
    }

    #[test]
    fn selection_mode_drag_selects_not_scrolls() {
        let mut recognizer = GestureRecognizer::new();
        recognizer.set_mode(InputMode::Selection);
        recognizer.on_touch_down(point(0, 0), 0);
        let gesture = recognizer
            .on_touch_move(point(SCROLL_THRESHOLD + 10, 5), 100)
            .expect("selection drag");
        assert!(
            matches!(gesture, Gesture::SelectionExtend { to } if to.x > 0),
            "selection mode must extend the selection, not scroll"
        );
        assert_eq!(recognizer.mode(), InputMode::Selection);
        // A separate quick tap (no drag) ends the selection.
        recognizer.on_touch_up(point(SCROLL_THRESHOLD + 10, 5), 150);
        recognizer.on_touch_down(point(5, 5), 200);
        recognizer.on_touch_up(point(5, 5), 250);
        assert_eq!(recognizer.mode(), InputMode::Normal);
    }

    #[test]
    fn extended_keys_do_not_conflict_with_scroll_or_selection() {
        // While a scroll is active, extended keys emit chords independently.
        let mut recognizer = GestureRecognizer::new();
        recognizer.on_touch_down(point(0, 0), 0);
        recognizer.on_touch_move(point(20, 0), 100);
        let mut keyboard = ExtendedKeyboard::new();
        // Press Ctrl, then Tab: the chord carries ctrl without disturbing the
        // active scroll gesture.
        assert_eq!(keyboard.press(ExtendedKey::Ctrl), None);
        let chord = keyboard.press(ExtendedKey::Tab).expect("chord");
        assert_eq!(
            chord,
            KeyChord {
                ctrl: true,
                alt: false,
                key: ExtendedKey::Tab
            }
        );
        // The scroll gesture continues unaffected.
        let next = recognizer.on_touch_move(point(30, 0), 150);
        assert!(matches!(next, Some(Gesture::Scroll { .. })));
        // In selection mode, Ctrl+Arrow emits a chord without conflict.
        let mut selection = GestureRecognizer::new();
        selection.set_mode(InputMode::Selection);
        let mut keyboard = ExtendedKeyboard::new();
        keyboard.press(ExtendedKey::Ctrl);
        let arrow = keyboard
            .press(ExtendedKey::Arrow(Direction::Left))
            .expect("arrow chord");
        assert!(arrow.ctrl);
        assert_eq!(arrow.key, ExtendedKey::Arrow(Direction::Left));
        assert_eq!(selection.mode(), InputMode::Selection);
        // Releasing Ctrl clears it.
        keyboard.release(ExtendedKey::Ctrl);
        assert!(!keyboard.ctrl_held());
        let tab = keyboard.press(ExtendedKey::Tab).unwrap();
        assert!(!tab.ctrl);
    }

    #[test]
    fn esc_and_alt_keys_emit_chords() {
        let mut keyboard = ExtendedKeyboard::new();
        keyboard.press(ExtendedKey::Alt);
        assert!(keyboard.alt_held());
        let esc = keyboard.press(ExtendedKey::Esc).expect("esc");
        assert!(esc.alt);
        assert_eq!(esc.key, ExtendedKey::Esc);
    }
}
