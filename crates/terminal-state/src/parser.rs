//! Terminal parser contract and event vocabulary (shared by the L1 screen
//! model and the L2 byte-stream parser).

/// A structured terminal event produced by the parser and consumed by the
/// screen model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseEvent {
    /// Printable text (already UTF-8 reassembled and coalesced).
    Text(String),
    /// CR.
    CarriageReturn,
    /// LF.
    LineFeed,
    /// BS.
    Backspace,
    /// `CSI Ps J` — erase display.
    EraseDisplay { mode: u16 },
    /// `CSI Ps K` — erase line.
    EraseLine { mode: u16 },
    /// `CSI Ps ; Ps H` (or f).
    CursorPosition { row: u16, col: u16 },
    /// `CSI Ps A/B/C/D`.
    CursorMove { row_delta: i16, col_delta: i16 },
    /// `CSI Ps m`.
    Sgr { params: Vec<u16> },
    /// `CSI ? Ps h/l`.
    SetMode {
        private_mode: bool,
        code: u16,
        enabled: bool,
    },
    /// `CSI Ps ; Ps r` — set scroll region (1-based; 0 = full).
    SetScrollRegion { top: u16, bottom: u16 },
    /// `OSC 0;title BEL/ST`.
    Title(String),
}

/// A structured parser diagnostic (stable code, no secret context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserDiagnostic {
    /// Stable code, e.g. `invalid_utf8`, `sequence_too_long`.
    pub code: String,
    /// i18n message key.
    pub message_key: String,
    /// Byte offset within the fed chunk (best effort).
    pub byte_offset: usize,
    /// Whether the condition is retryable after more input.
    pub retryable: bool,
}

impl ParserDiagnostic {
    /// Builds a diagnostic.
    pub fn new(code: &str, message_key: &str, byte_offset: usize, retryable: bool) -> Self {
        Self {
            code: code.to_owned(),
            message_key: message_key.to_owned(),
            byte_offset,
            retryable,
        }
    }
}

/// One feed batch: events plus any diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParseBatch {
    /// Monotonic feed sequence.
    pub sequence: u64,
    /// Parsed events.
    pub events: Vec<ParseEvent>,
    /// Diagnostics (never secrets).
    pub diagnostics: Vec<ParserDiagnostic>,
}

/// The injectable terminal parser contract.
pub trait TerminalParser {
    /// Feeds a byte chunk and returns the parsed batch.
    fn feed(&mut self, bytes: &[u8]) -> ParseBatch;
    /// Flushes incomplete state at end-of-stream and returns any final events.
    fn finish(&mut self) -> ParseBatch;
}
