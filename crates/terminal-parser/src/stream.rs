//! Bounded byte-stream to terminal-parser pipeline (T062).
//!
//! [`BoundedByteStreamParser`] consumes arbitrary byte chunks (network reads,
//! terminal PTY output) and produces structured [`ParseEvent`]s. UTF-8 is
//! reassembled across chunk boundaries (a maximum of 4 bytes are held),
//! ESC/CSI/OSC sequences may also be split across chunks, and every internal
//! buffer (UTF-8 pending, ESC/CSI/OSC, and the coalesced text buffer) is
//! hard-bounded so malicious input cannot grow memory without bound. The
//! fragmentation property (feeding byte-by-byte yields the same events as
//! feeding the whole stream) is verified by property tests.

/// Maximum bytes held for an incomplete UTF-8 sequence.
pub const MAX_UTF8_LEN: usize = 4;
/// Default cap for ESC/CSI/OSC sequence buffers.
pub const DEFAULT_MAX_SEQUENCE_LEN: usize = 4096;
/// Default cap for the coalesced text buffer.
pub const DEFAULT_MAX_TEXT_LEN: usize = 4096;

use terminal_state::parser::{ParseBatch, ParseEvent, ParserDiagnostic, TerminalParser};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Csi,
    Osc,
    DiscardUntilSt,
}
/// A bounded, fragmentation-safe byte-stream terminal parser.
pub struct BoundedByteStreamParser {
    /// Incomplete UTF-8 or in-progress ESC/CSI bytes.
    pending: Vec<u8>,
    /// Captured OSC payload.
    osc: Vec<u8>,
    /// Coalesced printable text (flushed on control/sequence or cap).
    text_buffer: String,
    /// Parser state.
    state: State,
    /// Whether the next Escape byte is the ST terminator of a pending OSC
    /// (set when `ESC` is seen inside an OSC payload).
    osc_st: bool,
    /// Monotonic feed sequence.
    sequence: u64,
    /// Hard cap for ESC/CSI/OSC buffers.
    max_sequence_len: usize,
    /// Hard cap for the coalesced text buffer.
    max_text_len: usize,
}

impl Default for BoundedByteStreamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl BoundedByteStreamParser {
    /// A parser with the default caps.
    pub fn new() -> Self {
        Self::with_caps(DEFAULT_MAX_SEQUENCE_LEN, DEFAULT_MAX_TEXT_LEN)
    }

    /// A parser with an explicit sequence cap (memory bound).
    pub fn with_max_sequence_len(max_sequence_len: usize) -> Self {
        Self::with_caps(max_sequence_len, DEFAULT_MAX_TEXT_LEN)
    }

    /// A parser with explicit sequence and text caps.
    pub fn with_caps(max_sequence_len: usize, max_text_len: usize) -> Self {
        Self {
            pending: Vec::with_capacity(8),
            osc: Vec::new(),
            text_buffer: String::new(),
            state: State::Ground,
            osc_st: false,
            sequence: 0,
            max_sequence_len,
            max_text_len,
        }
    }

    /// Current bytes held internally (the observable memory bound).
    pub fn pending_len(&self) -> usize {
        self.pending.len() + self.osc.len() + self.text_buffer.len()
    }

    /// The configured sequence cap.
    pub fn max_sequence_len(&self) -> usize {
        self.max_sequence_len
    }

    /// Flushes the coalesced text buffer as a `Text` event, if non-empty.
    fn flush_text(&mut self, events: &mut Vec<ParseEvent>) {
        if !self.text_buffer.is_empty() {
            events.push(ParseEvent::Text(std::mem::take(&mut self.text_buffer)));
        }
    }

    fn feed_byte(&mut self, byte: u8) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        match self.state {
            State::Ground => self.ground(byte),
            State::Escape => self.escape(byte),
            State::Csi => self.csi(byte),
            State::Osc => self.osc(byte),
            State::DiscardUntilSt => {
                if byte == 0x07 {
                    self.state = State::Ground;
                } else if byte == 0x1b {
                    self.state = State::Escape;
                }
                (Vec::new(), Vec::new())
            }
        }
    }

    fn ground(&mut self, byte: u8) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        let mut events = Vec::new();
        match byte {
            0x1b => {
                self.flush_text(&mut events);
                self.pending.clear();
                self.pending.push(byte);
                self.state = State::Escape;
            }
            b'\r' => {
                self.flush_text(&mut events);
                events.push(ParseEvent::CarriageReturn);
            }
            b'\n' => {
                self.flush_text(&mut events);
                events.push(ParseEvent::LineFeed);
            }
            0x08 => {
                self.flush_text(&mut events);
                events.push(ParseEvent::Backspace);
            }
            0x07 => {}
            _ => {
                self.pending.push(byte);
                match std::str::from_utf8(&self.pending) {
                    Ok(text) => {
                        self.text_buffer.push_str(text);
                        self.pending.clear();
                        if self.text_buffer.len() >= self.max_text_len {
                            self.flush_text(&mut events);
                        }
                    }
                    Err(error) if error.error_len().is_some() => {
                        let valid = error.valid_up_to();
                        if valid > 0 {
                            self.text_buffer.push_str(
                                std::str::from_utf8(&self.pending[..valid]).expect("valid prefix"),
                            );
                        }
                        self.text_buffer.push('\u{FFFD}');
                        self.pending.clear();
                        let diagnostic = ParserDiagnostic::new(
                            "invalid_utf8",
                            "terminal.parser.invalid_utf8",
                            0,
                            false,
                        );
                        if self.text_buffer.len() >= self.max_text_len {
                            self.flush_text(&mut events);
                        }
                        return (events, vec![diagnostic]);
                    }
                    Err(_) => {
                        // Incomplete UTF-8: hold up to MAX_UTF8_LEN bytes.
                        if self.pending.len() > MAX_UTF8_LEN {
                            self.text_buffer.push('\u{FFFD}');
                            self.pending.clear();
                            let diagnostic = ParserDiagnostic::new(
                                "invalid_utf8",
                                "terminal.parser.invalid_utf8",
                                0,
                                false,
                            );
                            return (events, vec![diagnostic]);
                        }
                    }
                }
            }
        }
        (events, Vec::new())
    }

    fn escape(&mut self, byte: u8) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        let mut events = Vec::new();
        self.pending.push(byte);
        match byte {
            b'[' => {
                self.state = State::Csi;
                self.osc_st = false;
            }
            b']' => {
                self.state = State::Osc;
                self.osc.clear();
                self.osc_st = false;
            }
            b'P' | b'_' | b'^' => {
                self.state = State::DiscardUntilSt;
                self.osc_st = false;
            }
            b'\\' if self.osc_st => {
                // ST (ESC \\) terminates the pending OSC.
                self.osc_st = false;
                self.pending.clear();
                self.state = State::Ground;
                let payload = std::mem::take(&mut self.osc);
                self.flush_text(&mut events);
                if let Some(event) = parse_osc(&payload) {
                    events.push(event);
                }
                return (events, Vec::new());
            }
            0x1b => {}
            _ => {
                // Two-character escape (e.g. ESC c): not in the T062 event set.
                self.pending.clear();
                self.state = State::Ground;
                self.osc_st = false;
            }
        }
        (events, Vec::new())
    }

    fn csi(&mut self, byte: u8) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        if self.pending.len() >= self.max_sequence_len {
            return self
                .reset_with_diagnostic("sequence_too_long", "terminal.parser.sequence_too_long");
        }
        self.pending.push(byte);
        if (0x40..=0x7e).contains(&byte) {
            let sequence = std::mem::take(&mut self.pending);
            self.state = State::Ground;
            let mut events = Vec::new();
            self.flush_text(&mut events);
            if let Some(event) = parse_csi(&sequence) {
                events.push(event);
            }
            (events, Vec::new())
        } else {
            (Vec::new(), Vec::new())
        }
    }

    fn osc(&mut self, byte: u8) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        if self.osc.len() >= self.max_sequence_len {
            return self
                .reset_with_diagnostic("sequence_too_long", "terminal.parser.sequence_too_long");
        }
        if byte == 0x07 {
            let payload = std::mem::take(&mut self.osc);
            self.state = State::Ground;
            self.pending.clear();
            self.osc_st = false;
            let mut events = Vec::new();
            self.flush_text(&mut events);
            if let Some(event) = parse_osc(&payload) {
                events.push(event);
            }
            (events, Vec::new())
        } else if byte == 0x1b {
            // Possible ST (ESC \\): the next byte decides in Escape state.
            self.state = State::Escape;
            self.osc_st = true;
            (Vec::new(), Vec::new())
        } else {
            self.osc.push(byte);
            (Vec::new(), Vec::new())
        }
    }

    fn reset_with_diagnostic(
        &mut self,
        code: &str,
        message_key: &str,
    ) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        self.pending.clear();
        self.osc.clear();
        self.state = State::Ground;
        self.osc_st = false;
        (
            Vec::new(),
            vec![ParserDiagnostic::new(code, message_key, 0, false)],
        )
    }
}

impl TerminalParser for BoundedByteStreamParser {
    fn feed(&mut self, bytes: &[u8]) -> ParseBatch {
        self.sequence += 1;
        let mut events = Vec::new();
        let mut diagnostics = Vec::new();
        for byte in bytes {
            let (mut new_events, mut new_diagnostics) = self.feed_byte(*byte);
            events.append(&mut new_events);
            diagnostics.append(&mut new_diagnostics);
        }
        ParseBatch {
            sequence: self.sequence,
            events,
            diagnostics,
        }
    }

    fn finish(&mut self) -> ParseBatch {
        self.sequence += 1;
        let mut events = Vec::new();
        let mut diagnostics = Vec::new();
        self.flush_text(&mut events);
        if !self.pending.is_empty() || !self.osc.is_empty() {
            diagnostics.push(ParserDiagnostic::new(
                "truncated_sequence",
                "terminal.parser.truncated_sequence",
                0,
                true,
            ));
        }
        self.pending.clear();
        self.osc.clear();
        self.state = State::Ground;
        ParseBatch {
            sequence: self.sequence,
            events,
            diagnostics,
        }
    }
}

/// Parses a completed CSI sequence (including the leading ESC [) into an event.
/// First value of the `index`-th parameter (or `default`).
fn csi_param(params: &[Vec<u16>], index: usize, default: u16) -> u16 {
    params
        .get(index)
        .and_then(|group| group.first())
        .copied()
        .unwrap_or(default)
}

fn parse_csi(sequence: &[u8]) -> Option<ParseEvent> {
    let body = &sequence[2..];
    let private = body
        .first()
        .copied()
        .filter(|b| matches!(b, b'?' | b'>' | b'<' | b'='));
    let start = if private.is_some() { 1 } else { 0 };
    // Parameters are `;`-separated; `:` separates sub-parameters within one
    // parameter (e.g. SGR `4:2` = double underline).
    let mut params: Vec<Vec<u16>> = Vec::new();
    let mut group: Vec<u16> = Vec::new();
    let mut current: Option<u16> = None;
    let mut index = start;
    while index < body.len() {
        let byte = body[index];
        if (0x30..=0x39).contains(&byte) {
            let digit = (byte - b'0') as u16;
            current = Some(
                current
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(digit),
            );
        } else if byte == b':' {
            group.push(current.unwrap_or(0));
            current = None;
        } else if byte == b';' {
            group.push(current.unwrap_or(0));
            params.push(std::mem::take(&mut group));
            current = None;
        }
        index += 1;
    }
    group.push(current.unwrap_or(0));
    params.push(group);
    let final_byte = *body.last()?;
    Some(match final_byte {
        b'J' => ParseEvent::EraseDisplay {
            mode: csi_param(&params, 0, 0),
        },
        b'K' => ParseEvent::EraseLine {
            mode: csi_param(&params, 0, 0),
        },
        b'H' | b'f' => ParseEvent::CursorPosition {
            row: csi_param(&params, 0, 1),
            col: csi_param(&params, 1, 1),
        },
        b'A' => ParseEvent::CursorMove {
            row_delta: -(csi_param(&params, 0, 1) as i16),
            col_delta: 0,
        },
        b'B' => ParseEvent::CursorMove {
            row_delta: csi_param(&params, 0, 1) as i16,
            col_delta: 0,
        },
        b'C' => ParseEvent::CursorMove {
            row_delta: 0,
            col_delta: csi_param(&params, 0, 1) as i16,
        },
        b'D' => ParseEvent::CursorMove {
            row_delta: 0,
            col_delta: -(csi_param(&params, 0, 1) as i16),
        },
        b'm' => ParseEvent::Sgr { params },
        b'r' => ParseEvent::SetScrollRegion {
            top: csi_param(&params, 0, 1),
            bottom: csi_param(&params, 1, 0),
        },
        b'h' | b'l' => ParseEvent::SetMode {
            private_mode: private == Some(b'?'),
            code: csi_param(&params, 0, 0),
            enabled: final_byte == b'h',
        },
        _ => return None,
    })
}

/// Parses an OSC payload into a structured event (T066).
///
/// OSC 0/2 set the window title, OSC 7 sets the working directory, OSC 9 and
/// OSC 777;notify request a desktop notification; unknown codes are ignored.
fn parse_osc(payload: &[u8]) -> Option<ParseEvent> {
    let text = String::from_utf8_lossy(payload);
    let (code, rest) = text.split_once(';').unwrap_or(("", text.as_ref()));
    match code {
        "0" | "2" => Some(ParseEvent::Title(rest.to_owned())),
        "7" => Some(ParseEvent::WorkingDirectory(rest.to_owned())),
        "9" => Some(ParseEvent::Notification {
            summary: rest.to_owned(),
            body: String::new(),
        }),
        "777" => {
            let mut parts = rest.splitn(3, ';');
            match (parts.next(), parts.next(), parts.next()) {
                (Some("notify"), summary, body) => Some(ParseEvent::Notification {
                    summary: summary.unwrap_or_default().to_owned(),
                    body: body.unwrap_or_default().to_owned(),
                }),
                _ => Some(ParseEvent::Notification {
                    summary: rest.to_owned(),
                    body: String::new(),
                }),
            }
        }
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::{
        BoundedByteStreamParser, ParseEvent, ParserDiagnostic, TerminalParser,
        DEFAULT_MAX_SEQUENCE_LEN, MAX_UTF8_LEN,
    };

    fn collect(
        parser: &mut impl TerminalParser,
        bytes: &[u8],
    ) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        let batch = parser.feed(bytes);
        (batch.events, batch.diagnostics)
    }

    fn collect_split(
        parser: &mut impl TerminalParser,
        bytes: &[u8],
        widths: &[usize],
    ) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        let mut events = Vec::new();
        let mut diagnostics = Vec::new();
        let mut start = 0usize;
        for width in widths {
            if start >= bytes.len() {
                break;
            }
            let end = (start + width).min(bytes.len());
            let batch = parser.feed(&bytes[start..end]);
            events.extend(batch.events);
            diagnostics.extend(batch.diagnostics);
            start = end;
        }
        if start < bytes.len() {
            let batch = parser.feed(&bytes[start..]);
            events.extend(batch.events);
            diagnostics.extend(batch.diagnostics);
        }
        (events, diagnostics)
    }

    fn corpus() -> Vec<u8> {
        b"hello\r\n\x1b[2J\x1b[31mred\x1b[2;5H!\x1b]0;demo\x07\xe9\x9d\x9b\xe5\xb3\x8c".to_vec()
    }

    #[test]
    fn whole_equals_fragmented() {
        let input = corpus();
        let (one_events, one_diags) = collect(&mut BoundedByteStreamParser::new(), &input);
        let (split_events, split_diags) = collect_split(
            &mut BoundedByteStreamParser::new(),
            &input,
            &[2, 1, 4, 3, 2, 1, 5, 2, 3, 99],
        );
        assert_eq!(one_events, split_events, "events must be chunk-independent");
        assert_eq!(one_diags, split_diags);
        assert!(one_diags.is_empty());

        // Byte-by-byte feeding is also equivalent.
        let (byte_events, byte_diags) = collect_split(
            &mut BoundedByteStreamParser::new(),
            &input,
            &vec![1; input.len()],
        );
        assert_eq!(one_events, byte_events);
        assert_eq!(one_diags, byte_diags);
    }

    #[test]
    fn fragmented_utf8_across_chunks() {
        let mut parser = BoundedByteStreamParser::new();
        // U+1F600 (emoji) is 4 bytes: F0 9F 98 80
        let emoji = [0xf0u8, 0x9f, 0x98, 0x80];
        for (i, byte) in emoji.iter().enumerate() {
            let batch = parser.feed(&[*byte]);
            assert!(batch.diagnostics.is_empty());
            assert!(batch.events.is_empty(), "incomplete UTF-8 must not emit");
            assert!(
                parser.pending_len() <= MAX_UTF8_LEN,
                "pending must stay tiny"
            );
            let _ = i;
        }
        // A control flushes the reassembled character.
        let batch = parser.feed(b"\n");
        assert!(batch.diagnostics.is_empty());
        assert_eq!(
            batch.events,
            vec![
                ParseEvent::Text("\u{1F600}".to_owned()),
                ParseEvent::LineFeed
            ]
        );
        assert_eq!(parser.pending_len(), 0);
    }

    #[test]
    fn fragmented_csi_across_chunks() {
        let mut parser = BoundedByteStreamParser::new();
        let csi = b"\x1b[2;5H";
        for byte in &csi[..csi.len() - 1] {
            let batch = parser.feed(&[*byte]);
            assert!(batch.events.is_empty());
            assert!(batch.diagnostics.is_empty());
        }
        let batch = parser.feed(&[csi[csi.len() - 1]]);
        assert!(batch.diagnostics.is_empty());
        assert_eq!(
            batch.events,
            vec![ParseEvent::CursorPosition { row: 2, col: 5 }]
        );
    }

    #[test]
    fn fragmented_osc_across_chunks() {
        let mut parser = BoundedByteStreamParser::new();
        let osc = b"\x1b]0;hello world\x07";
        for (i, byte) in osc.iter().enumerate() {
            let batch = parser.feed(&[*byte]);
            assert!(batch.diagnostics.is_empty());
            if i < osc.len() - 1 {
                assert!(batch.events.is_empty());
            } else {
                assert_eq!(
                    batch.events,
                    vec![ParseEvent::Title("hello world".to_owned())]
                );
            }
        }
    }

    #[test]
    fn basic_event_sequence() {
        let mut parser = BoundedByteStreamParser::new();
        let (events, diags) = collect(
            &mut parser,
            b"hi\r\n\x1b[2J\x1b[31m\x1b]0;demo\x07\xe9\x9d\x9b\n",
        );
        assert!(diags.is_empty());
        assert_eq!(
            events,
            vec![
                ParseEvent::Text("hi".to_owned()),
                ParseEvent::CarriageReturn,
                ParseEvent::LineFeed,
                ParseEvent::EraseDisplay { mode: 2 },
                ParseEvent::Sgr {
                    params: vec![vec![31]]
                },
                ParseEvent::Title("demo".to_owned()),
                ParseEvent::Text("\u{975B}".to_owned()),
                ParseEvent::LineFeed,
            ]
        );
    }

    #[test]
    fn osc_working_directory_and_notifications() {
        let mut parser = BoundedByteStreamParser::new();
        let (events, diags) = collect(
            &mut parser,
            b"\x1b]7;file:///home/user/project\x07\
              \x1b]9;build done\x07\
              \x1b]777;notify;deploy;2 of 3 ok\x07\
              \x1b]99;ignored\x07",
        );
        assert!(diags.is_empty());
        assert_eq!(
            events,
            vec![
                ParseEvent::WorkingDirectory("file:///home/user/project".to_owned()),
                ParseEvent::Notification {
                    summary: "build done".to_owned(),
                    body: String::new(),
                },
                ParseEvent::Notification {
                    summary: "deploy".to_owned(),
                    body: "2 of 3 ok".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn osc_terminated_by_st_is_finalized() {
        let mut parser = BoundedByteStreamParser::new();
        let (events, diags) = collect(&mut parser, b"\x1b]0;st-title\x1b\\");
        assert!(diags.is_empty());
        assert_eq!(events, vec![ParseEvent::Title("st-title".to_owned())]);
        // The parser recovers and continues after ST.
        let (events, diags) = collect(&mut parser, b"ok\n");
        assert!(diags.is_empty());
        assert_eq!(
            events,
            vec![ParseEvent::Text("ok".to_owned()), ParseEvent::LineFeed]
        );
    }

    #[test]
    fn invalid_utf8_is_diagnosed_and_replaced() {
        let mut parser = BoundedByteStreamParser::new();
        let (events, diags) = collect(&mut parser, b"a\xffb\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "invalid_utf8");
        // Coalesced text: "a" + replacement + "b", flushed by the LF.
        assert_eq!(
            events,
            vec![
                ParseEvent::Text("a\u{FFFD}b".to_owned()),
                ParseEvent::LineFeed
            ]
        );
        // The parser recovers and continues.
        let (events, diags) = collect(&mut parser, b"ok\n");
        assert!(diags.is_empty());
        assert_eq!(
            events,
            vec![ParseEvent::Text("ok".to_owned()), ParseEvent::LineFeed]
        );
    }

    #[test]
    fn malicious_input_memory_is_bounded() {
        let mut parser = BoundedByteStreamParser::with_caps(1024, 4096);
        let mut state = 0x12345678u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut input = Vec::with_capacity(1 << 20);
        for _ in 0..(1 << 20) {
            let byte = match next() % 8 {
                0 => 0x1b,
                1 => b'[',
                2 => b';',
                3 => b'0' + (next() % 10) as u8,
                4 => 0x07,
                5 => 0xff,
                _ => (next() % 256) as u8,
            };
            input.push(byte);
        }
        let bound = 1024 + MAX_UTF8_LEN + 4096;
        let mut offset = 0usize;
        while offset < input.len() {
            let width = (next() % 257) as usize + 1;
            let end = (offset + width).min(input.len());
            let _ = parser.feed(&input[offset..end]);
            assert!(
                parser.pending_len() <= bound,
                "pending {} exceeds bound {bound}",
                parser.pending_len()
            );
            offset = end;
        }
        let _ = parser.finish();
        // The parser is still usable after the hostile stream.
        let (events, diags) = collect(&mut parser, b"ok\n");
        assert!(diags.is_empty());
        assert_eq!(
            events,
            vec![ParseEvent::Text("ok".to_owned()), ParseEvent::LineFeed]
        );
    }

    #[test]
    fn oversized_sequence_is_diagnosed_and_bounded() {
        let mut parser = BoundedByteStreamParser::with_caps(64, 4096);
        let mut long = Vec::new();
        long.push(0x1b);
        long.push(b'[');
        for _ in 0..200 {
            long.push(b'1');
            long.push(b';');
        }
        long.push(b'm');
        let (events, diags) = collect(&mut parser, &long);
        assert!(
            diags.iter().any(|d| d.code == "sequence_too_long"),
            "expected sequence_too_long, got {diags:?}"
        );
        // The parser reset and did not buffer the whole attack; the tail after
        // the reset is held in the bounded text buffer.
        assert!(parser.pending_len() <= 64 + MAX_UTF8_LEN + 4096);
        assert!(events.is_empty());
        let _ = parser.finish();
        let (events, diags) = collect(&mut parser, b"ok\n");
        assert!(diags.is_empty());
        assert_eq!(
            events,
            vec![ParseEvent::Text("ok".to_owned()), ParseEvent::LineFeed]
        );
    }

    #[test]
    fn finish_reports_truncated_sequence() {
        let mut parser = BoundedByteStreamParser::new();
        let _ = parser.feed(b"\xe9"); // incomplete 2-byte UTF-8
        assert!(parser.pending_len() > 0);
        let batch = parser.finish();
        assert!(
            batch
                .diagnostics
                .iter()
                .any(|d| d.code == "truncated_sequence"),
            "finish must report truncated state"
        );
        assert_eq!(parser.pending_len(), 0);
    }

    #[test]
    fn sequence_cap_is_configurable() {
        assert_eq!(
            BoundedByteStreamParser::new().max_sequence_len(),
            DEFAULT_MAX_SEQUENCE_LEN
        );
        assert_eq!(
            BoundedByteStreamParser::with_max_sequence_len(16).max_sequence_len(),
            16
        );
        assert_eq!(
            BoundedByteStreamParser::with_caps(8, 32).max_sequence_len(),
            8
        );
    }

    #[test]
    fn text_buffer_is_capped_and_flushed() {
        // A pure text flood must flush in bounded chunks, never growing memory.
        let mut parser = BoundedByteStreamParser::with_caps(64, 32);
        let flood = vec![b'a'; 1000];
        let (mut events, _) = collect(&mut parser, &flood);
        let batch = parser.finish();
        events.extend(batch.events);
        let total: usize = events
            .iter()
            .map(|e| match e {
                ParseEvent::Text(t) => t.len(),
                _ => 0,
            })
            .sum();
        assert_eq!(total, 1000);
        assert!(parser.pending_len() <= 32, "text buffer must stay capped");
    }
}
