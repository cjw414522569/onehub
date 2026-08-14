//! Reference implementations for the versioned terminal contract.
//!
//! The crate intentionally has no SSH, UI, persistence, or platform
//! dependency. Its tests prove that the four public layers are replaceable
//! while exchanging only bounded batches and structured values.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalOutputChunk {
    pub stream_id: u64,
    pub sequence: u64,
    pub bytes: Vec<u8>,
    pub end_of_chunk: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParserDiagnostic {
    pub code: String,
    pub message_key: String,
    pub byte_offset: usize,
    pub retryable: bool,
}

impl ParserDiagnostic {
    fn new(code: &str, message_key: &str, byte_offset: usize) -> Self {
        Self {
            code: code.to_owned(),
            message_key: message_key.to_owned(),
            byte_offset,
            retryable: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseEvent {
    Text(String),
    CarriageReturn,
    LineFeed,
    Backspace,
    EraseDisplay {
        mode: u16,
    },
    EraseLine {
        mode: u16,
    },
    CursorPosition {
        row: u16,
        col: u16,
    },
    CursorMove {
        row_delta: i16,
        col_delta: i16,
    },
    Sgr {
        params: Vec<u16>,
    },
    SetMode {
        private_mode: bool,
        code: u16,
        enabled: bool,
    },
    Title(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseBatch {
    pub stream_id: u64,
    pub sequence: u64,
    pub events: Vec<ParseEvent>,
    pub diagnostics: Vec<ParserDiagnostic>,
}

pub trait TerminalParser {
    fn feed(&mut self, chunk: TerminalOutputChunk) -> ParseBatch;
    fn finish(&mut self) -> ParseBatch;
}

pub struct Utf8VtParser {
    stream_id: Option<u64>,
    pending: Vec<u8>,
    text_buffer: String,
    last_sequence: Option<u64>,
}

impl Utf8VtParser {
    pub fn new() -> Self {
        Self {
            stream_id: None,
            pending: Vec::new(),
            text_buffer: String::new(),
            last_sequence: None,
        }
    }

    fn make_batch(
        &self,
        sequence: u64,
        events: Vec<ParseEvent>,
        diagnostics: Vec<ParserDiagnostic>,
    ) -> ParseBatch {
        ParseBatch {
            stream_id: self.stream_id.unwrap_or_default(),
            sequence,
            events,
            diagnostics,
        }
    }

    fn drain(&mut self) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        let mut events = Vec::new();
        let mut diagnostics = Vec::new();
        while !self.pending.is_empty() {
            match self.pending[0] {
                0x1b => match parse_escape(&self.pending) {
                    EscapeParse::Incomplete => break,
                    EscapeParse::Parsed(consumed, event, diagnostic) => {
                        flush_text(&mut self.text_buffer, &mut events);
                        self.pending.drain(..consumed);
                        if let Some(event) = event {
                            events.push(event);
                        }
                        if let Some(diagnostic) = diagnostic {
                            diagnostics.push(diagnostic);
                        }
                    }
                },
                b'\r' => {
                    flush_text(&mut self.text_buffer, &mut events);
                    self.pending.remove(0);
                    events.push(ParseEvent::CarriageReturn);
                }
                b'\n' => {
                    flush_text(&mut self.text_buffer, &mut events);
                    self.pending.remove(0);
                    events.push(ParseEvent::LineFeed);
                }
                0x08 => {
                    flush_text(&mut self.text_buffer, &mut events);
                    self.pending.remove(0);
                    events.push(ParseEvent::Backspace);
                }
                _ => {
                    let end = self
                        .pending
                        .iter()
                        .position(|byte| matches!(*byte, 0x1b | b'\r' | b'\n' | 0x08))
                        .unwrap_or(self.pending.len());
                    match std::str::from_utf8(&self.pending[..end]) {
                        Ok(text) => {
                            self.text_buffer.push_str(text);
                            self.pending.drain(..end);
                        }
                        Err(error) if error.error_len().is_none() => break,
                        Err(error) => {
                            let valid = error.valid_up_to();
                            if valid > 0 {
                                let text = String::from_utf8(self.pending[..valid].to_vec())
                                    .expect("valid UTF-8 prefix");
                                self.text_buffer.push_str(&text);
                                self.pending.drain(..valid);
                            } else {
                                self.pending.remove(0);
                                self.text_buffer.push('�');
                                diagnostics.push(ParserDiagnostic::new(
                                    "invalid_utf8",
                                    "terminal.parser.invalid_utf8",
                                    0,
                                ));
                            }
                        }
                    }
                }
            }
        }
        (events, diagnostics)
    }
}

impl Default for Utf8VtParser {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalParser for Utf8VtParser {
    fn feed(&mut self, chunk: TerminalOutputChunk) -> ParseBatch {
        let mut diagnostics = Vec::new();
        if let Some(stream_id) = self.stream_id {
            if stream_id != chunk.stream_id {
                diagnostics.push(ParserDiagnostic::new(
                    "stream_mismatch",
                    "terminal.parser.stream_mismatch",
                    0,
                ));
            }
        } else {
            self.stream_id = Some(chunk.stream_id);
        }
        if let Some(last) = self.last_sequence {
            if chunk.sequence <= last {
                diagnostics.push(ParserDiagnostic::new(
                    "non_monotonic_sequence",
                    "terminal.parser.non_monotonic_sequence",
                    0,
                ));
            }
        }
        self.last_sequence = Some(
            self.last_sequence
                .map_or(chunk.sequence, |last| last.max(chunk.sequence)),
        );
        self.pending.extend_from_slice(&chunk.bytes);
        let (events, mut parsed_diagnostics) = self.drain();
        diagnostics.append(&mut parsed_diagnostics);
        self.make_batch(chunk.sequence, events, diagnostics)
    }

    fn finish(&mut self) -> ParseBatch {
        let sequence = self.last_sequence.unwrap_or_default();
        let mut diagnostics = Vec::new();
        let mut events = Vec::new();
        flush_text(&mut self.text_buffer, &mut events);
        if !self.pending.is_empty() {
            let bytes = std::mem::take(&mut self.pending);
            self.text_buffer.push_str(&String::from_utf8_lossy(&bytes));
            flush_text(&mut self.text_buffer, &mut events);
            diagnostics.push(ParserDiagnostic::new(
                "truncated_sequence",
                "terminal.parser.truncated_sequence",
                0,
            ));
        }
        self.make_batch(sequence, events, diagnostics)
    }
}

fn flush_text(buffer: &mut String, events: &mut Vec<ParseEvent>) {
    if !buffer.is_empty() {
        events.push(ParseEvent::Text(std::mem::take(buffer)));
    }
}

enum EscapeParse {
    Incomplete,
    Parsed(usize, Option<ParseEvent>, Option<ParserDiagnostic>),
}

fn parse_escape(bytes: &[u8]) -> EscapeParse {
    if bytes.len() < 2 {
        return EscapeParse::Incomplete;
    }
    match bytes[1] {
        b'[' => parse_csi(bytes),
        b']' => parse_osc(bytes),
        _ => EscapeParse::Parsed(
            2,
            None,
            Some(ParserDiagnostic::new(
                "unsupported_escape",
                "terminal.parser.unsupported_escape",
                0,
            )),
        ),
    }
}

fn parse_csi(bytes: &[u8]) -> EscapeParse {
    let Some(final_index) = bytes[2..]
        .iter()
        .position(|byte| (0x40..=0x7e).contains(byte))
        .map(|index| index + 2)
    else {
        return EscapeParse::Incomplete;
    };
    let (private_mode, params) = parse_csi_params(&bytes[2..final_index]);
    let final_byte = bytes[final_index];
    let event = match final_byte {
        b'J' => Some(ParseEvent::EraseDisplay {
            mode: params.first().copied().unwrap_or(0),
        }),
        b'K' => Some(ParseEvent::EraseLine {
            mode: params.first().copied().unwrap_or(0),
        }),
        b'H' | b'f' => Some(ParseEvent::CursorPosition {
            row: params.first().copied().unwrap_or(1).max(1),
            col: params.get(1).copied().unwrap_or(1).max(1),
        }),
        b'A' => Some(ParseEvent::CursorMove {
            row_delta: -(params.first().copied().unwrap_or(1).max(1) as i16),
            col_delta: 0,
        }),
        b'B' => Some(ParseEvent::CursorMove {
            row_delta: params.first().copied().unwrap_or(1).max(1) as i16,
            col_delta: 0,
        }),
        b'C' => Some(ParseEvent::CursorMove {
            row_delta: 0,
            col_delta: params.first().copied().unwrap_or(1).max(1) as i16,
        }),
        b'D' => Some(ParseEvent::CursorMove {
            row_delta: 0,
            col_delta: -(params.first().copied().unwrap_or(1).max(1) as i16),
        }),
        b'm' => Some(ParseEvent::Sgr { params }),
        b'h' | b'l' => Some(ParseEvent::SetMode {
            private_mode,
            code: params.first().copied().unwrap_or_default(),
            enabled: final_byte == b'h',
        }),
        _ => None,
    };
    let diagnostic = event
        .is_none()
        .then(|| ParserDiagnostic::new("unsupported_csi", "terminal.parser.unsupported_csi", 0));
    EscapeParse::Parsed(final_index + 1, event, diagnostic)
}

fn parse_osc(bytes: &[u8]) -> EscapeParse {
    let mut end = None;
    for index in 2..bytes.len() {
        if bytes[index] == 0x07 {
            end = Some((index, index + 1));
            break;
        }
        if bytes[index] == 0x1b {
            if bytes.get(index + 1) == Some(&b'\\') {
                end = Some((index, index + 2));
                break;
            }
            return EscapeParse::Incomplete;
        }
    }
    let Some((end_index, consumed)) = end else {
        return EscapeParse::Incomplete;
    };
    let payload = String::from_utf8_lossy(&bytes[2..end_index]);
    let title = payload
        .split_once(';')
        .map(|(_, value)| value)
        .unwrap_or(payload.as_ref());
    EscapeParse::Parsed(consumed, Some(ParseEvent::Title(title.to_owned())), None)
}

fn parse_csi_params(bytes: &[u8]) -> (bool, Vec<u16>) {
    let private_mode = bytes.first() == Some(&b'?');
    let bytes = if private_mode { &bytes[1..] } else { bytes };
    let params = bytes
        .split(|byte| *byte == b';')
        .map(|part| {
            if part.is_empty() {
                0
            } else {
                String::from_utf8_lossy(part).parse::<u16>().unwrap_or(0)
            }
        })
        .collect();
    (private_mode, params)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCell {
    pub ch: char,
    pub fg: u8,
    pub bg: u8,
    pub bold: bool,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: 7,
            bg: 0,
            bold: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRow {
    pub cells: Vec<TerminalCell>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorState {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeSet {
    pub bracketed_paste: bool,
    pub application_cursor: bool,
    pub alternate_screen: bool,
}

impl Default for ModeSet {
    fn default() -> Self {
        Self {
            bracketed_paste: false,
            application_cursor: false,
            alternate_screen: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub stream_id: u64,
    pub sequence: u64,
    pub rows: Vec<TerminalRow>,
    pub cursor: CursorState,
    pub modes: ModeSet,
    pub title: String,
    pub scrollback_range: (u64, u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelResult {
    pub sequence: u64,
    pub diagnostics: Vec<ParserDiagnostic>,
}

pub trait ScreenModel {
    fn apply(&mut self, batch: &ParseBatch) -> ModelResult;
    fn snapshot(&self) -> TerminalSnapshot;
}

#[derive(Clone)]
struct ModelState {
    stream_id: u64,
    cols: u16,
    rows: u16,
    grid: Vec<Vec<TerminalCell>>,
    cursor: CursorState,
    modes: ModeSet,
    title: String,
    scrollback_range: (u64, u64),
    last_sequence: u64,
}

impl ModelState {
    fn new(stream_id: u64, cols: u16, rows: u16) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            stream_id,
            cols,
            rows,
            grid: vec![vec![TerminalCell::default(); cols as usize]; rows as usize],
            cursor: CursorState {
                row: 0,
                col: 0,
                visible: true,
            },
            modes: ModeSet::default(),
            title: String::new(),
            scrollback_range: (0, rows as u64),
            last_sequence: 0,
        }
    }

    fn snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            stream_id: self.stream_id,
            sequence: self.last_sequence,
            rows: self
                .grid
                .iter()
                .cloned()
                .map(|cells| TerminalRow { cells })
                .collect(),
            cursor: self.cursor.clone(),
            modes: self.modes.clone(),
            title: self.title.clone(),
            scrollback_range: self.scrollback_range,
        }
    }

    fn apply_batch(&mut self, batch: &ParseBatch) -> ModelResult {
        let mut diagnostics = batch.diagnostics.clone();
        if batch.stream_id != self.stream_id {
            diagnostics.push(ParserDiagnostic::new(
                "stream_mismatch",
                "terminal.model.stream_mismatch",
                0,
            ));
        }
        if batch.sequence < self.last_sequence {
            diagnostics.push(ParserDiagnostic::new(
                "non_monotonic_sequence",
                "terminal.model.non_monotonic_sequence",
                0,
            ));
            return ModelResult {
                sequence: self.last_sequence,
                diagnostics,
            };
        }
        self.last_sequence = batch.sequence;
        for event in &batch.events {
            self.apply_event(event);
        }
        ModelResult {
            sequence: self.last_sequence,
            diagnostics,
        }
    }

    fn apply_event(&mut self, event: &ParseEvent) {
        match event {
            ParseEvent::Text(text) => {
                for ch in text.chars() {
                    self.put_char(ch);
                }
            }
            ParseEvent::CarriageReturn => self.cursor.col = 0,
            ParseEvent::LineFeed => self.line_feed(),
            ParseEvent::Backspace => self.cursor.col = self.cursor.col.saturating_sub(1),
            ParseEvent::EraseDisplay { mode } => self.erase_display(*mode),
            ParseEvent::EraseLine { mode } => self.erase_line(*mode),
            ParseEvent::CursorPosition { row, col } => {
                self.cursor.row = row.saturating_sub(1).min(self.rows - 1);
                self.cursor.col = col.saturating_sub(1).min(self.cols - 1);
            }
            ParseEvent::CursorMove {
                row_delta,
                col_delta,
            } => {
                self.cursor.row = add_signed(self.cursor.row, *row_delta, self.rows - 1);
                self.cursor.col = add_signed(self.cursor.col, *col_delta, self.cols - 1);
            }
            ParseEvent::Sgr { params } => self.apply_sgr(params),
            ParseEvent::SetMode {
                private_mode,
                code,
                enabled,
            } => {
                if *private_mode && *code == 2004 {
                    self.modes.bracketed_paste = *enabled;
                } else if *private_mode && *code == 1 {
                    self.modes.application_cursor = *enabled;
                } else if *private_mode && (*code == 47 || *code == 1049) {
                    self.modes.alternate_screen = *enabled;
                }
            }
            ParseEvent::Title(title) => self.title = title.clone(),
        }
    }

    fn put_char(&mut self, ch: char) {
        let cell = &mut self.grid[self.cursor.row as usize][self.cursor.col as usize];
        cell.ch = ch;
        if self.cursor.col + 1 >= self.cols {
            self.cursor.col = 0;
            self.line_feed();
        } else {
            self.cursor.col += 1;
        }
    }

    fn line_feed(&mut self) {
        if self.cursor.row + 1 >= self.rows {
            self.grid.remove(0);
            self.grid
                .push(vec![TerminalCell::default(); self.cols as usize]);
            self.scrollback_range.0 += 1;
            self.scrollback_range.1 += 1;
        } else {
            self.cursor.row += 1;
        }
    }

    fn erase_display(&mut self, mode: u16) {
        match mode {
            2 | 3 => {
                self.grid =
                    vec![vec![TerminalCell::default(); self.cols as usize]; self.rows as usize];
                self.cursor.row = 0;
                self.cursor.col = 0;
            }
            0 => {
                self.erase_line(0);
                for row in (self.cursor.row as usize + 1)..self.grid.len() {
                    self.grid[row].fill(TerminalCell::default());
                }
            }
            1 => {
                for row in 0..self.cursor.row as usize {
                    self.grid[row].fill(TerminalCell::default());
                }
                self.erase_line(1);
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let row = &mut self.grid[self.cursor.row as usize];
        match mode {
            0 => row[self.cursor.col as usize..].fill(TerminalCell::default()),
            1 => row[..=self.cursor.col as usize].fill(TerminalCell::default()),
            2 => row.fill(TerminalCell::default()),
            _ => {}
        }
    }

    fn apply_sgr(&mut self, params: &[u16]) {
        if params.is_empty() || params.contains(&0) {
            for row in &mut self.grid {
                for cell in row {
                    cell.fg = 7;
                    cell.bg = 0;
                    cell.bold = false;
                }
            }
        }
    }
}

fn add_signed(value: u16, delta: i16, max: u16) -> u16 {
    if delta.is_negative() {
        value.saturating_sub(delta.unsigned_abs()).min(max)
    } else {
        value.saturating_add(delta as u16).min(max)
    }
}

pub struct ReferenceScreenModel {
    state: ModelState,
}

impl ReferenceScreenModel {
    pub fn new(stream_id: u64, cols: u16, rows: u16) -> Self {
        Self {
            state: ModelState::new(stream_id, cols, rows),
        }
    }
}

impl ScreenModel for ReferenceScreenModel {
    fn apply(&mut self, batch: &ParseBatch) -> ModelResult {
        self.state.apply_batch(batch)
    }

    fn snapshot(&self) -> TerminalSnapshot {
        self.state.snapshot()
    }
}

pub struct MirrorScreenModel {
    state: ModelState,
}

impl MirrorScreenModel {
    pub fn new(stream_id: u64, cols: u16, rows: u16) -> Self {
        Self {
            state: ModelState::new(stream_id, cols, rows),
        }
    }
}

impl ScreenModel for MirrorScreenModel {
    fn apply(&mut self, batch: &ParseBatch) -> ModelResult {
        self.state.apply_batch(batch)
    }

    fn snapshot(&self) -> TerminalSnapshot {
        self.state.snapshot()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderCellRun {
    pub row: u16,
    pub col: u16,
    pub cells: Vec<TerminalCell>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderCursor {
    pub cursor: CursorState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderOp {
    Fill(RenderCellRun),
    Copy {
        source_row: u16,
        source_col: u16,
        dest_row: u16,
        dest_col: u16,
        width: u16,
        height: u16,
    },
    Clear,
    Cursor(RenderCursor),
    Title(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderBatch {
    pub from_sequence: u64,
    pub to_sequence: u64,
    pub operations: Vec<RenderOp>,
}

pub trait RenderDiff {
    fn diff(&self, previous: &TerminalSnapshot, next: &TerminalSnapshot) -> RenderBatch;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StableRenderDiff;

impl RenderDiff for StableRenderDiff {
    fn diff(&self, previous: &TerminalSnapshot, next: &TerminalSnapshot) -> RenderBatch {
        let mut operations = Vec::new();
        let old_width = previous.rows.first().map_or(0, |row| row.cells.len());
        let new_width = next.rows.first().map_or(0, |row| row.cells.len());
        if previous.rows.len() != next.rows.len() || old_width != new_width {
            operations.push(RenderOp::Clear);
            for (row_index, row) in next.rows.iter().enumerate() {
                if !row.cells.is_empty() {
                    operations.push(RenderOp::Fill(RenderCellRun {
                        row: row_index as u16,
                        col: 0,
                        cells: row.cells.clone(),
                    }));
                }
            }
        } else {
            for (row_index, (old_row, new_row)) in previous.rows.iter().zip(&next.rows).enumerate()
            {
                let mut index = 0usize;
                while index < new_row.cells.len() {
                    if old_row.cells[index] == new_row.cells[index] {
                        index += 1;
                        continue;
                    }
                    let start = index;
                    while index < new_row.cells.len()
                        && old_row.cells[index] != new_row.cells[index]
                    {
                        index += 1;
                    }
                    operations.push(RenderOp::Fill(RenderCellRun {
                        row: row_index as u16,
                        col: start as u16,
                        cells: new_row.cells[start..index].to_vec(),
                    }));
                }
            }
        }
        if previous.cursor != next.cursor {
            operations.push(RenderOp::Cursor(RenderCursor {
                cursor: next.cursor.clone(),
            }));
        }
        if previous.title != next.title {
            operations.push(RenderOp::Title(next.title.clone()));
        }
        RenderBatch {
            from_sequence: previous.sequence,
            to_sequence: next.sequence,
            operations,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Function(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Other(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MouseInput {
    pub button: MouseButton,
    pub col: u16,
    pub row: u16,
    pub pressed: bool,
    pub modifiers: Modifiers,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Text(String),
    Key { code: KeyCode, modifiers: Modifiers },
    Paste(String),
    Mouse(MouseInput),
    Resize { cols: u16, rows: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputBatch {
    pub stream_id: u64,
    pub sequence: u64,
    pub events: Vec<InputEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedInput {
    pub stream_id: u64,
    pub sequence: u64,
    pub bytes: Vec<u8>,
}

pub trait InputEncoder {
    fn encode(&self, batch: &InputBatch) -> EncodedInput;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AnsiInputEncoder;

impl InputEncoder for AnsiInputEncoder {
    fn encode(&self, batch: &InputBatch) -> EncodedInput {
        let mut bytes = Vec::new();
        for event in &batch.events {
            match event {
                InputEvent::Text(text) => bytes.extend_from_slice(text.as_bytes()),
                InputEvent::Key { code, modifiers } => encode_key(code, modifiers, &mut bytes),
                InputEvent::Paste(text) => {
                    bytes.extend_from_slice(b"\x1b[200~");
                    bytes.extend_from_slice(text.as_bytes());
                    bytes.extend_from_slice(b"\x1b[201~");
                }
                InputEvent::Mouse(mouse) => encode_mouse(mouse, &mut bytes),
                InputEvent::Resize { cols, rows } => {
                    bytes.extend_from_slice(format!("\x1b[8;{rows};{cols}t").as_bytes());
                }
            }
        }
        EncodedInput {
            stream_id: batch.stream_id,
            sequence: batch.sequence,
            bytes,
        }
    }
}

fn modifier_parameter(modifiers: &Modifiers) -> u8 {
    1 + u8::from(modifiers.shift)
        + 2 * u8::from(modifiers.alt)
        + 4 * u8::from(modifiers.ctrl)
        + 8 * u8::from(modifiers.meta)
}

fn encode_key(code: &KeyCode, modifiers: &Modifiers, bytes: &mut Vec<u8>) {
    let parameter = modifier_parameter(modifiers);
    match code {
        KeyCode::Char(ch) if modifiers.ctrl && ch.is_ascii_alphabetic() => {
            bytes.push((*ch as u8).to_ascii_lowercase() & 0x1f);
        }
        KeyCode::Char(ch) => {
            if modifiers.alt {
                bytes.push(0x1b);
            }
            let mut buffer = [0; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
        }
        KeyCode::Enter => bytes.extend_from_slice(b"\r"),
        KeyCode::Escape => bytes.extend_from_slice(b"\x1b"),
        KeyCode::Backspace => bytes.extend_from_slice(b"\x7f"),
        KeyCode::Tab => bytes.extend_from_slice(b"\t"),
        KeyCode::ArrowUp => encode_csi_key('A', parameter, bytes),
        KeyCode::ArrowDown => encode_csi_key('B', parameter, bytes),
        KeyCode::ArrowRight => encode_csi_key('C', parameter, bytes),
        KeyCode::ArrowLeft => encode_csi_key('D', parameter, bytes),
        KeyCode::Home => encode_csi_key('H', parameter, bytes),
        KeyCode::End => encode_csi_key('F', parameter, bytes),
        KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
        KeyCode::Function(number) => {
            let code = match number {
                1 => "11~",
                2 => "12~",
                3 => "13~",
                4 => "14~",
                5 => "15~",
                6 => "17~",
                7 => "18~",
                8 => "19~",
                9 => "20~",
                10 => "21~",
                _ => "23~",
            };
            bytes.extend_from_slice(format!("\x1b[{code}").as_bytes());
        }
    }
}

fn encode_csi_key(final_byte: char, parameter: u8, bytes: &mut Vec<u8>) {
    if parameter == 1 {
        bytes.extend_from_slice(format!("\x1b[{final_byte}").as_bytes());
    } else {
        bytes.extend_from_slice(format!("\x1b[1;{parameter}{final_byte}").as_bytes());
    }
}

fn encode_mouse(mouse: &MouseInput, bytes: &mut Vec<u8>) {
    let button = match mouse.button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Other(value) => value,
    } + if mouse.modifiers.shift { 4 } else { 0 }
        + if mouse.modifiers.alt { 8 } else { 0 }
        + if mouse.modifiers.ctrl { 16 } else { 0 };
    let suffix = if mouse.pressed { 'M' } else { 'm' };
    bytes.extend_from_slice(
        format!("\x1b[<{};{};{}{}", button, mouse.col, mouse.row, suffix).as_bytes(),
    );
}

impl fmt::Display for ParserDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.code, self.message_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_parser(
        mut parser: impl TerminalParser,
        chunks: Vec<Vec<u8>>,
    ) -> (Vec<ParseEvent>, Vec<ParserDiagnostic>) {
        let mut events = Vec::new();
        let mut diagnostics = Vec::new();
        for (index, bytes) in chunks.into_iter().enumerate() {
            let batch = parser.feed(TerminalOutputChunk {
                stream_id: 7,
                sequence: index as u64 + 1,
                bytes,
                end_of_chunk: true,
            });
            events.extend(batch.events);
            diagnostics.extend(batch.diagnostics);
        }
        let final_batch = parser.finish();
        events.extend(final_batch.events);
        diagnostics.extend(final_batch.diagnostics);
        (events, diagnostics)
    }

    #[test]
    fn parser_chunking_is_equivalent_and_reports_structured_diagnostics() {
        let input = "hello\r\n\x1b[2J\x1b[31mred\x1b]0;demo\x07界🙂"
            .as_bytes()
            .to_vec();
        let one = collect_parser(Utf8VtParser::new(), vec![input.clone()]);
        let mut chunks = Vec::new();
        let mut start = 0usize;
        for width in [2usize, 1, 4, 3, 2, 1, 5, 2, 3, 99] {
            if start >= input.len() {
                break;
            }
            let end = (start + width).min(input.len());
            chunks.push(input[start..end].to_vec());
            start = end;
        }
        let split = collect_parser(Utf8VtParser::new(), chunks);
        assert_eq!(one.0, split.0);
        assert_eq!(one.1, split.1);
        assert!(one.1.is_empty());

        let (_, diagnostics) = collect_parser(Utf8VtParser::new(), vec![vec![0xff]]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "invalid_utf8");
        assert_eq!(diagnostics[0].message_key, "terminal.parser.invalid_utf8");
    }

    #[test]
    fn two_screen_model_wrappers_produce_deterministic_snapshots() {
        let mut parser = Utf8VtParser::new();
        let batch = parser.feed(TerminalOutputChunk {
            stream_id: 7,
            sequence: 1,
            bytes: b"abc\x1b[2;3H!\x1b]0;title\x07".to_vec(),
            end_of_chunk: true,
        });
        let mut reference = ReferenceScreenModel::new(7, 8, 3);
        let mut mirror = MirrorScreenModel::new(7, 8, 3);
        assert!(reference.apply(&batch).diagnostics.is_empty());
        assert!(mirror.apply(&batch).diagnostics.is_empty());
        assert_eq!(reference.snapshot(), mirror.snapshot());
        assert_eq!(reference.snapshot(), reference.snapshot());

        let older = ParseBatch {
            stream_id: 7,
            sequence: 0,
            events: vec![ParseEvent::Text("old".to_owned())],
            diagnostics: Vec::new(),
        };
        let result = reference.apply(&older);
        assert_eq!(result.sequence, 1);
        assert_eq!(result.diagnostics[0].code, "non_monotonic_sequence");
    }

    #[test]
    fn render_diff_is_empty_for_same_snapshot_and_stable_for_one_change() {
        let mut model = ReferenceScreenModel::new(7, 4, 2);
        let empty = ParseBatch {
            stream_id: 7,
            sequence: 1,
            events: Vec::new(),
            diagnostics: Vec::new(),
        };
        model.apply(&empty);
        let previous = model.snapshot();
        let batch = ParseBatch {
            stream_id: 7,
            sequence: 2,
            events: vec![ParseEvent::Text("X".to_owned())],
            diagnostics: Vec::new(),
        };
        model.apply(&batch);
        let next = model.snapshot();
        let diff = StableRenderDiff;
        assert!(diff.diff(&previous, &previous).operations.is_empty());
        let changed = diff.diff(&previous, &next);
        assert_eq!(changed.from_sequence, 1);
        assert_eq!(changed.to_sequence, 2);
        assert_eq!(changed.operations.len(), 2);
        assert!(matches!(changed.operations[0], RenderOp::Fill(_)));
        assert!(matches!(changed.operations[1], RenderOp::Cursor(_)));
        assert_eq!(changed, diff.diff(&previous, &next));
    }

    #[test]
    fn input_encoder_is_batch_only_stable_and_covers_all_event_kinds() {
        let batch = InputBatch {
            stream_id: 9,
            sequence: 4,
            events: vec![
                InputEvent::Text("hi".to_owned()),
                InputEvent::Key {
                    code: KeyCode::ArrowUp,
                    modifiers: Modifiers {
                        shift: true,
                        ..Modifiers::default()
                    },
                },
                InputEvent::Paste("paste".to_owned()),
                InputEvent::Mouse(MouseInput {
                    button: MouseButton::Left,
                    col: 2,
                    row: 3,
                    pressed: true,
                    modifiers: Modifiers::default(),
                }),
                InputEvent::Resize { cols: 80, rows: 24 },
            ],
        };
        let encoder: &dyn InputEncoder = &AnsiInputEncoder;
        let first = encoder.encode(&batch);
        let second = encoder.encode(&batch);
        assert_eq!(first, second);
        assert_eq!(first.stream_id, 9);
        assert_eq!(first.sequence, 4);
        let text = String::from_utf8(first.bytes).expect("stable ANSI bytes");
        assert!(text.starts_with("hi\x1b[1;2A"));
        assert!(text.contains("\x1b[200~paste\x1b[201~"));
        assert!(text.contains("\x1b[<0;2;3M"));
        assert!(text.ends_with("\x1b[8;24;80t"));
    }
}
