#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # terminal-state
//!
//! Shared terminal parser contract (L1 event vocabulary) and the primary /
//! alternate screen model with cursor, scroll region, DEC/ANSI modes
//! (T062/T063), plus the locked Unicode width policy and grapheme
//! segmentation used by the model (T064).

pub mod color;
pub mod hyperlink;
pub mod input;
pub mod osc;
pub mod parser;
pub mod screen;
pub mod scrollback;
pub mod selection;
pub mod unicode;

pub use color::{Palette, Rgb};
pub use hyperlink::{effective_host, scheme_of, HyperlinkPolicy, HyperlinkReview};
pub use input::{
    encode_focus, encode_key, encode_mouse, encode_paste, Key, KeyEvent, KeyboardProtocol,
    Modifiers, MouseAction, MouseButton, MouseEncoding, MouseEvent, MouseMode,
};
pub use osc::{Notification, OscPolicy};
pub use parser::{ParseBatch, ParseEvent, ParserDiagnostic, TerminalParser};
pub use screen::{Modes, ScreenBuffer, ScreenModel};
pub use scrollback::{Scrollback, ScrollbackConfig, ScrollbackDumpPolicy};
pub use selection::{cell_selection_text, is_word_char, word_bounds, Selection, SelectionMode};
pub use unicode::{grapheme_clusters, WidthPolicy, UNICODE_VERSION};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "terminal-state";
