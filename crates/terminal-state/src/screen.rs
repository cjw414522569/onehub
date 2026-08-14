//! Primary / alternate screen, cursor, scroll region, and mode state (T063),
//! with Unicode grapheme / wide-character handling (T064).
//!
//! [`ScreenModel`] consumes the [`ParseEvent`]s produced by the L2 byte-stream
//! parser and maintains two buffers (primary and alternate), a cursor with a
//! saved position, a scroll region, DEC/ANSI modes, SGR attributes, and a
//! configurable [`WidthPolicy`]. Text is written grapheme-by-grapheme, so
//! combining sequences and ZWJ emoji occupy a single cell with the width of
//! their base, and wide clusters mark a continuation cell. The visible state
//! is exposed as a [`TerminalSnapshot`] for the renderer.

use core_protocol::terminal::{
    CursorState, Hyperlink, TerminalCell, TerminalColor, TerminalRow, TerminalSnapshot,
    TerminalStyle, UnderlineStyle,
};

use crate::hyperlink::HyperlinkPolicy;
use crate::input::{KeyboardProtocol, MouseMode};
use crate::osc::{Notification, OscPolicy};
use crate::parser::{ParseBatch, ParseEvent, ParserDiagnostic};
use crate::scrollback::{Scrollback, ScrollbackConfig, ScrollbackDumpPolicy};
use crate::unicode::{grapheme_clusters, WidthPolicy};

/// DEC/ANSI modes tracked by the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes {
    /// DECIM / SM ?4 — insert mode.
    pub insert: bool,
    /// DECAWM / SM ?7 — autowrap (default on).
    pub autowrap: bool,
    /// DECOM / SM ?6 — origin mode (cursor relative to scroll region).
    pub origin: bool,
    /// DECCKM / SM ?1 — application cursor keys.
    pub app_cursor_keys: bool,
    /// DECKPAM / SM ?66 — application keypad.
    pub app_keypad: bool,
    /// DECTCEM / SM ?25 — cursor visible (default on).
    pub cursor_visible: bool,
    /// ?2004 — bracketed paste.
    pub bracketed_paste: bool,
    /// ?5 — reverse video.
    pub reverse_video: bool,
    /// ?1000/?1002/?1003 — mouse tracking mode.
    pub mouse_mode: MouseMode,
    /// ?1006 — SGR mouse coordinate encoding.
    pub mouse_sgr: bool,
    /// ?1004 — focus events.
    pub focus_events: bool,
    /// Negotiated keyboard protocol (xterm / modifyOtherKeys / kitty).
    pub keyboard_protocol: KeyboardProtocol,
    /// ?47 / ?1049 — alternate screen active.
    pub alternate_screen: bool,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            insert: false,
            autowrap: true,
            origin: false,
            app_cursor_keys: false,
            app_keypad: false,
            cursor_visible: true,
            bracketed_paste: false,
            reverse_video: false,
            mouse_mode: MouseMode::Off,
            mouse_sgr: false,
            focus_events: false,
            keyboard_protocol: KeyboardProtocol::Xterm,
            alternate_screen: false,
        }
    }
}

/// One screen buffer: grid, cursor, scroll region, saved cursor, SGR style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenBuffer {
    cells: Vec<Vec<TerminalCell>>,
    rows: usize,
    cols: usize,
    cursor_row: usize,
    cursor_col: usize,
    saved_row: usize,
    saved_col: usize,
    scroll_top: usize,
    scroll_bottom: usize,
    style: TerminalStyle,
    pending_wrap: bool,
    current_hyperlink: Option<Hyperlink>,
}

impl ScreenBuffer {
    /// Creates a blank buffer.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            cells: vec![vec![TerminalCell::empty(); cols]; rows],
            rows,
            cols,
            cursor_row: 0,
            cursor_col: 0,
            saved_row: 0,
            saved_col: 0,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            style: TerminalStyle::default(),
            pending_wrap: false,
            current_hyperlink: None,
        }
    }

    /// Cursor row (0-based).
    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    /// Cursor column (0-based).
    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    /// Scroll region top (0-based).
    pub fn scroll_top(&self) -> usize {
        self.scroll_top
    }

    /// Scroll region bottom (0-based, inclusive).
    pub fn scroll_bottom(&self) -> usize {
        self.scroll_bottom
    }

    /// Current SGR style.
    pub fn style(&self) -> &TerminalStyle {
        &self.style
    }

    fn linefeed(&mut self) -> Option<TerminalRow> {
        if self.cursor_row == self.scroll_bottom {
            let scrolled = self.scroll_up(1);
            self.pending_wrap = false;
            scrolled
        } else {
            if self.cursor_row + 1 < self.rows {
                self.cursor_row += 1;
            }
            self.pending_wrap = false;
            None
        }
    }

    /// Scrolls the region up by `count` lines (text at the bottom clears).
    /// Returns the row that scrolled off the screen top (when the region
    /// starts at row 0) for scrollback capture.
    fn scroll_up(&mut self, count: usize) -> Option<TerminalRow> {
        let count = count.min(self.scroll_bottom - self.scroll_top + 1);
        let scrolled = if self.scroll_top == 0 {
            Some(TerminalRow {
                cells: std::mem::take(&mut self.cells[self.scroll_top]),
            })
        } else {
            None
        };
        for row in self.scroll_top..=self.scroll_bottom {
            let source = row + count;
            if source <= self.scroll_bottom {
                self.cells[row] = std::mem::take(&mut self.cells[source]);
            } else {
                self.cells[row] = vec![TerminalCell::empty(); self.cols];
            }
        }
        self.pending_wrap = false;
        scrolled
    }

    /// Carriage return.
    fn carriage_return(&mut self) {
        self.cursor_col = 0;
    }

    /// Backspace.
    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    /// Writes one grapheme cluster at the cursor with wrap / insert
    /// semantics.
    ///
    /// A wide cluster (width 2 under the active [`WidthPolicy`]) occupies its
    /// cell plus a marked continuation cell; a zero-width cluster (a combining
    /// mark arriving on its own) merges into the cell before the cursor.
    fn put_grapheme(
        &mut self,
        cluster: &str,
        modes: &Modes,
        policy: WidthPolicy,
    ) -> Option<TerminalRow> {
        let width = policy.cluster_width(cluster);
        if width == 0 {
            if self.cursor_col > 0 {
                if let Some(prev) = self.cells[self.cursor_row].get_mut(self.cursor_col - 1) {
                    prev.text.get_or_insert_with(String::new).push_str(cluster);
                }
            }
            return None;
        }
        let mut scrolled = None;
        if self.pending_wrap && modes.autowrap {
            scrolled = self.linefeed();
            self.cursor_col = 0;
        }
        let row = self.cursor_row;
        let col = self.cursor_col;
        if modes.insert && col < self.cols {
            self.cells[row].insert(col, TerminalCell::empty());
            self.cells[row].pop();
            if width >= 2 && col + 1 < self.cols {
                self.cells[row].insert(col + 1, TerminalCell::empty());
                self.cells[row].pop();
            }
        }
        self.break_wide_at(row, col);
        if row < self.rows && col < self.cols {
            let mut cell = TerminalCell::cluster(cluster);
            cell.style = self.style.clone();
            cell.hyperlink = self.current_hyperlink.clone();
            self.cells[row][col] = cell;
            if width >= 2 {
                let end = (col + width).min(self.cols);
                for cont in (col + 1)..end {
                    let mut continuation = TerminalCell::wide_continuation(self.style.clone());
                    continuation.hyperlink = self.current_hyperlink.clone();
                    self.cells[row][cont] = continuation;
                }
            }
        }
        if col + width >= self.cols {
            self.cursor_col = self.cols.saturating_sub(1);
            if modes.autowrap {
                self.pending_wrap = true;
            }
        } else {
            self.cursor_col = col + width;
        }
        scrolled
    }

    /// Clears the cell at `(row, col)`; if it was the continuation half of a
    /// wide cluster, the base cell at `col - 1` is cleared as well so no
    /// orphaned wide pair survives an overwrite or partial erase.
    fn break_wide_at(&mut self, row: usize, col: usize) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        if self.cells[row][col].wide_continuation && col > 0 {
            self.cells[row][col - 1] = TerminalCell::empty();
        }
        self.cells[row][col] = TerminalCell::empty();
    }

    /// Sets the active hyperlink; subsequent cells carry it.
    fn set_hyperlink(&mut self, hyperlink: Hyperlink) {
        self.current_hyperlink = Some(hyperlink);
    }

    /// Ends the active hyperlink (OSC 8 with an empty URI).
    fn clear_hyperlink(&mut self) {
        self.current_hyperlink = None;
    }

    /// Erase display: 0 cursor->end, 1 start->cursor, 2 all, 3 all (+scrollback
    /// note; no scrollback in T063).
    fn erase_display(&mut self, mode: u16) {
        match mode {
            0 => {
                for col in self.cursor_col..self.cols {
                    self.break_wide_at(self.cursor_row, col);
                }
                for row in (self.cursor_row + 1)..self.rows {
                    self.cells[row] = vec![TerminalCell::empty(); self.cols];
                }
            }
            1 => {
                for col in 0..=self.cursor_col {
                    self.break_wide_at(self.cursor_row, col);
                }
                for row in 0..self.cursor_row {
                    self.cells[row] = vec![TerminalCell::empty(); self.cols];
                }
            }
            _ => {
                for row in 0..self.rows {
                    self.cells[row] = vec![TerminalCell::empty(); self.cols];
                }
            }
        }
    }

    /// Erase line: 0 cursor->end, 1 start->cursor, 2 all.
    fn erase_line(&mut self, mode: u16) {
        match mode {
            0 => {
                for col in self.cursor_col..self.cols {
                    self.break_wide_at(self.cursor_row, col);
                }
            }
            1 => {
                for col in 0..=self.cursor_col {
                    self.break_wide_at(self.cursor_row, col);
                }
            }
            _ => {
                self.cells[self.cursor_row] = vec![TerminalCell::empty(); self.cols];
            }
        }
    }

    /// Moves the cursor by a row/col delta, respecting the scroll region.
    /// Returns the row scrolled off the top (if any) for scrollback capture.
    fn move_cursor(&mut self, row_delta: i16, col_delta: i16) -> Option<TerminalRow> {
        let row = self.cursor_row as i64 + row_delta as i64;
        let scrolled = if row < 0 {
            self.cursor_row = 0;
            None
        } else if row as usize > self.scroll_bottom {
            let overflow = row as usize - self.scroll_bottom;
            let scrolled = self.scroll_up(overflow);
            self.cursor_row = self.scroll_bottom;
            scrolled
        } else {
            self.cursor_row = row as usize;
            None
        };
        let col = self.cursor_col as i64 + col_delta as i64;
        self.cursor_col = col.clamp(0, (self.cols as i64) - 1) as usize;
        scrolled
    }

    /// Positions the cursor at 1-based `row`,`col` (origin mode maps to the
    /// scroll region).
    fn position_cursor(&mut self, row: u16, col: u16, modes: &Modes) {
        let (row, col) = if modes.origin {
            let base = self.scroll_top;
            let max = self.scroll_bottom - self.scroll_top + 1;
            let row = (row as usize).saturating_sub(1).min(max.saturating_sub(1));
            (
                base + row,
                (col as usize).saturating_sub(1).min(self.cols - 1),
            )
        } else {
            (
                (row as usize).saturating_sub(1).min(self.rows - 1),
                (col as usize).saturating_sub(1).min(self.cols - 1),
            )
        };
        self.cursor_row = row;
        self.cursor_col = col;
        self.pending_wrap = false;
    }

    /// Saves the cursor position.
    fn save_cursor(&mut self) {
        self.saved_row = self.cursor_row;
        self.saved_col = self.cursor_col;
    }

    /// Restores the cursor position.
    fn restore_cursor(&mut self) {
        self.cursor_row = self.saved_row.min(self.rows - 1);
        self.cursor_col = self.saved_col.min(self.cols - 1);
        self.pending_wrap = false;
    }

    /// Applies an SGR sequence to the current style (T063 basic subset,
    /// T065 completed: 16/256/truecolor, inverse/dim, underline styles).
    ///
    /// Each parameter may carry `:`-separated sub-parameters (e.g. `4:2` for
    /// double underline); `38;5;n` / `38;2;r;g;b` color selectors span the
    /// following `;`-separated parameters.
    fn apply_sgr(&mut self, params: &[Vec<u16>]) {
        if params.is_empty() || params.iter().any(|g| g.first() == Some(&0)) {
            self.style = TerminalStyle::default();
        }
        let mut index = 0usize;
        while index < params.len() {
            let group = &params[index];
            let param = group.first().copied().unwrap_or(0);
            match param {
                1 => self.style.bold = true,
                2 => self.style.dim = true,
                3 => self.style.italic = true,
                4 => {
                    // Underline with optional style sub-parameter (4:0..4:5).
                    match group.get(1).copied() {
                        Some(0) => {
                            self.style.underline = false;
                            self.style.underline_style = UnderlineStyle::None;
                        }
                        Some(1) | None => {
                            self.style.underline = true;
                            self.style.underline_style = UnderlineStyle::Single;
                        }
                        Some(2) => {
                            self.style.underline = true;
                            self.style.underline_style = UnderlineStyle::Double;
                        }
                        Some(3) => {
                            self.style.underline = true;
                            self.style.underline_style = UnderlineStyle::Curly;
                        }
                        Some(4) => {
                            self.style.underline = true;
                            self.style.underline_style = UnderlineStyle::Dotted;
                        }
                        Some(5) => {
                            self.style.underline = true;
                            self.style.underline_style = UnderlineStyle::Dashed;
                        }
                        Some(_) => {
                            self.style.underline = true;
                            self.style.underline_style = UnderlineStyle::Single;
                        }
                    }
                }
                7 => self.style.inverse = true,
                21 => {
                    self.style.underline = true;
                    self.style.underline_style = UnderlineStyle::Double;
                }
                22 => {
                    self.style.bold = false;
                    self.style.dim = false;
                }
                23 => self.style.italic = false,
                24 => {
                    self.style.underline = false;
                    self.style.underline_style = UnderlineStyle::None;
                }
                27 => self.style.inverse = false,
                30..=37 => self.style.fg = TerminalColor::Indexed((param - 30) as u8),
                39 => self.style.fg = TerminalColor::Default,
                40..=47 => self.style.bg = TerminalColor::Indexed((param - 40) as u8),
                49 => self.style.bg = TerminalColor::Default,
                90..=97 => self.style.fg = TerminalColor::Indexed((param - 90 + 8) as u8),
                100..=107 => self.style.bg = TerminalColor::Indexed((param - 100 + 8) as u8),
                38 | 48 => {
                    // 38;5;n (256-color) / 38;2;r;g;b (truecolor).
                    let sub = params.get(index + 1).and_then(|g| g.first()).copied();
                    match sub {
                        Some(5) => {
                            if let Some(color) =
                                params.get(index + 2).and_then(|g| g.first()).copied()
                            {
                                let color = TerminalColor::Indexed(color as u8);
                                if param == 38 {
                                    self.style.fg = color;
                                } else {
                                    self.style.bg = color;
                                }
                                index += 2;
                            }
                        }
                        Some(2) => {
                            if let (Some(r), Some(g), Some(b)) = (
                                params.get(index + 2).and_then(|g| g.first()).copied(),
                                params.get(index + 3).and_then(|g| g.first()).copied(),
                                params.get(index + 4).and_then(|g| g.first()).copied(),
                            ) {
                                let color = TerminalColor::TrueColor {
                                    r: r as u8,
                                    g: g as u8,
                                    b: b as u8,
                                };
                                if param == 38 {
                                    self.style.fg = color;
                                } else {
                                    self.style.bg = color;
                                }
                                index += 4;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }
}

/// The full terminal model: primary + alternate buffers, modes, title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenModel {
    primary: ScreenBuffer,
    alternate: ScreenBuffer,
    modes: Modes,
    title: Option<String>,
    working_directory: Option<String>,
    notification: Option<Notification>,
    osc_policy: OscPolicy,
    hyperlink_policy: HyperlinkPolicy,
    scrollback: Scrollback,
    scrollback_config: ScrollbackConfig,
    scrollback_dump_policy: ScrollbackDumpPolicy,
    stream_id: u64,
    sequence: u64,
    width_policy: WidthPolicy,
}

impl ScreenModel {
    /// Creates a model with `rows` x `cols` buffers.
    pub fn new(stream_id: u64, rows: usize, cols: usize) -> Self {
        Self {
            primary: ScreenBuffer::new(rows, cols),
            alternate: ScreenBuffer::new(rows, cols),
            modes: Modes::default(),
            title: None,
            working_directory: None,
            notification: None,
            osc_policy: OscPolicy::default(),
            hyperlink_policy: HyperlinkPolicy::default(),
            scrollback: Scrollback::new(ScrollbackConfig::default().max_lines),
            scrollback_config: ScrollbackConfig::default(),
            scrollback_dump_policy: ScrollbackDumpPolicy::default(),
            stream_id,
            sequence: 0,
            width_policy: WidthPolicy::default(),
        }
    }

    /// The current modes.
    pub fn modes(&self) -> &Modes {
        &self.modes
    }

    /// The configured Unicode width policy.
    pub fn width_policy(&self) -> WidthPolicy {
        self.width_policy
    }

    /// Sets the width policy (Unicode / East Asian / Legacy).
    pub fn set_width_policy(&mut self, policy: WidthPolicy) {
        self.width_policy = policy;
    }

    /// Whether the alternate screen is active.
    pub fn active_is_alternate(&self) -> bool {
        self.modes.alternate_screen
    }

    /// The active buffer.
    pub fn active_buffer(&self) -> &ScreenBuffer {
        if self.modes.alternate_screen {
            &self.alternate
        } else {
            &self.primary
        }
    }

    /// The primary buffer.
    pub fn primary(&self) -> &ScreenBuffer {
        &self.primary
    }

    /// The alternate buffer.
    pub fn alternate(&self) -> &ScreenBuffer {
        &self.alternate
    }

    /// The window title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The OSC 7 working directory (policy-sanitized).
    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    /// The most recent policy-approved notification, if any.
    pub fn notification(&self) -> Option<&Notification> {
        self.notification.as_ref()
    }

    /// Takes (clears) the most recent notification; the UI layer consumes it.
    pub fn take_notification(&mut self) -> Option<Notification> {
        self.notification.take()
    }

    /// The active OSC policy.
    pub fn osc_policy(&self) -> &OscPolicy {
        &self.osc_policy
    }

    /// Sets the OSC policy (title / working-directory / notification gating).
    pub fn set_osc_policy(&mut self, policy: OscPolicy) {
        self.osc_policy = policy;
    }

    /// The active hyperlink policy.
    pub fn hyperlink_policy(&self) -> &HyperlinkPolicy {
        &self.hyperlink_policy
    }

    /// Sets the hyperlink policy (scheme whitelist + length cap).
    pub fn set_hyperlink_policy(&mut self, policy: HyperlinkPolicy) {
        self.hyperlink_policy = policy;
    }

    /// The scrollback ring buffer (primary screen only).
    pub fn scrollback(&self) -> &Scrollback {
        &self.scrollback
    }

    /// Number of retained scrollback lines.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// The active scrollback configuration.
    pub fn scrollback_config(&self) -> &ScrollbackConfig {
        &self.scrollback_config
    }

    /// Sets the scrollback capacity; retained overflow is evicted.
    pub fn set_scrollback_config(&mut self, config: ScrollbackConfig) {
        self.scrollback_config = config;
        self.scrollback.set_max_lines(config.max_lines);
    }

    /// The disk dump policy (off by default).
    pub fn scrollback_dump_policy(&self) -> &ScrollbackDumpPolicy {
        &self.scrollback_dump_policy
    }

    /// Sets the disk dump policy.
    pub fn set_scrollback_dump_policy(&mut self, policy: ScrollbackDumpPolicy) {
        self.scrollback_dump_policy = policy;
    }

    /// Renders a bounded text dump of the scrollback, or `None` when dumps
    /// are not permitted.
    pub fn dump_scrollback(&self, cols: usize) -> Option<String> {
        self.scrollback_dump_policy.dump(&self.scrollback, cols)
    }

    /// Resizes both buffers (preserving content where possible).
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.primary.resize(rows, cols);
        self.alternate.resize(rows, cols);
    }

    /// Applies a parser batch; returns any model diagnostics.
    pub fn apply_batch(&mut self, batch: &ParseBatch) -> Vec<ParserDiagnostic> {
        self.sequence = self.sequence.max(batch.sequence);
        for event in &batch.events {
            self.apply_event(event);
        }
        batch.diagnostics.clone()
    }

    /// Applies one event.
    pub fn apply_event(&mut self, event: &ParseEvent) {
        let buffer = if self.modes.alternate_screen {
            &mut self.alternate
        } else {
            &mut self.primary
        };
        match event {
            ParseEvent::Text(text) => {
                for cluster in grapheme_clusters(text) {
                    if let Some(row) = buffer.put_grapheme(cluster, &self.modes, self.width_policy)
                    {
                        if !self.modes.alternate_screen {
                            self.scrollback.push(row);
                        }
                    }
                }
            }
            ParseEvent::CarriageReturn => buffer.carriage_return(),
            ParseEvent::LineFeed => {
                if let Some(row) = buffer.linefeed() {
                    if !self.modes.alternate_screen {
                        self.scrollback.push(row);
                    }
                }
            }
            ParseEvent::Backspace => buffer.backspace(),
            ParseEvent::EraseDisplay { mode } => buffer.erase_display(*mode),
            ParseEvent::EraseLine { mode } => buffer.erase_line(*mode),
            ParseEvent::CursorPosition { row, col } => {
                buffer.position_cursor(*row, *col, &self.modes)
            }
            ParseEvent::CursorMove {
                row_delta,
                col_delta,
            } => {
                if let Some(row) = buffer.move_cursor(*row_delta, *col_delta) {
                    if !self.modes.alternate_screen {
                        self.scrollback.push(row);
                    }
                }
            }
            ParseEvent::Sgr { params } => buffer.apply_sgr(params),
            ParseEvent::SetMode {
                private_mode,
                code,
                enabled,
            } => self.set_mode(*private_mode, *code, *enabled),
            ParseEvent::SetScrollRegion { top, bottom } => {
                buffer.set_scroll_region(*top, *bottom, &self.modes);
            }
            ParseEvent::Title(title) => {
                if let Some(title) = self.osc_policy.sanitize_title(title) {
                    self.title = Some(title);
                }
            }
            ParseEvent::WorkingDirectory(directory) => {
                if let Some(directory) = self.osc_policy.sanitize_working_directory(directory) {
                    self.working_directory = Some(directory);
                }
            }
            ParseEvent::Notification { summary, body } => {
                self.notification = self.osc_policy.sanitize_notification(summary, body);
            }
            ParseEvent::Hyperlink { id, url } => {
                if url.is_empty() {
                    buffer.clear_hyperlink();
                } else if self.hyperlink_policy.can_open(url) {
                    buffer.set_hyperlink(Hyperlink {
                        id: id.clone(),
                        url: url.clone(),
                    });
                }
            }
        }
    }

    fn set_mode(&mut self, private_mode: bool, code: u16, enabled: bool) {
        if !private_mode {
            // ANSI modes (SM/RM without ?).
            return;
        }
        match code {
            1 => self.modes.app_cursor_keys = enabled,
            4 => self.modes.insert = enabled,
            5 => self.modes.reverse_video = enabled,
            6 => self.modes.origin = enabled,
            7 => self.modes.autowrap = enabled,
            25 => self.modes.cursor_visible = enabled,
            47 => self.modes.alternate_screen = enabled,
            66 => self.modes.app_keypad = enabled,
            1000 => {
                self.modes.mouse_mode = if enabled {
                    MouseMode::Buttons
                } else {
                    MouseMode::Off
                };
            }
            1002 => {
                self.modes.mouse_mode = if enabled {
                    MouseMode::Drag
                } else {
                    MouseMode::Off
                };
            }
            1003 => {
                self.modes.mouse_mode = if enabled {
                    MouseMode::Motion
                } else {
                    MouseMode::Off
                };
            }
            1004 => self.modes.focus_events = enabled,
            1006 => self.modes.mouse_sgr = enabled,
            2004 => self.modes.bracketed_paste = enabled,
            1049 => {
                if enabled {
                    self.primary.save_cursor();
                    self.modes.alternate_screen = true;
                    self.alternate.clear_all();
                } else {
                    self.modes.alternate_screen = false;
                    self.primary.restore_cursor();
                }
            }
            _ => {}
        }
    }

    /// The visible snapshot for the renderer.
    pub fn snapshot(&self) -> TerminalSnapshot {
        let buffer = self.active_buffer();
        TerminalSnapshot {
            stream_id: self.stream_id,
            sequence: self.sequence,
            rows: buffer
                .cells
                .iter()
                .map(|cells| TerminalRow {
                    cells: cells.clone(),
                })
                .collect(),
            cursor: CursorState {
                row: buffer.cursor_row as u16,
                col: buffer.cursor_col as u16,
                visible: self.modes.cursor_visible,
            },
            title: self.title.clone(),
            working_directory: self.working_directory.clone(),
            scrollback_start: self.scrollback.len() as u64,
            extensions: Vec::new(),
        }
    }
}

impl ScreenBuffer {
    fn clear_all(&mut self) {
        for row in &mut self.cells {
            *row = vec![TerminalCell::empty(); self.cols];
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.pending_wrap = false;
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.cells.resize(rows, Vec::new());
        for row in &mut self.cells {
            row.resize(cols, TerminalCell::empty());
        }
        self.rows = rows;
        self.cols = cols;
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
        self.scroll_bottom = rows.saturating_sub(1);
        self.scroll_top = self.scroll_top.min(rows.saturating_sub(1));
    }

    /// Sets the scroll region from 1-based CSI params (0 = full screen).
    fn set_scroll_region(&mut self, top: u16, bottom: u16, modes: &Modes) {
        let top = if top == 0 {
            0
        } else {
            (top as usize).saturating_sub(1)
        };
        let bottom = if bottom == 0 {
            self.rows.saturating_sub(1)
        } else {
            (bottom as usize)
                .saturating_sub(1)
                .min(self.rows.saturating_sub(1))
        };
        self.scroll_top = top.min(bottom);
        self.scroll_bottom = bottom;
        if modes.origin {
            self.cursor_row = self.scroll_top;
            self.cursor_col = 0;
        }
    }
}
#[cfg(test)]
mod tests {
    use core_protocol::terminal::TerminalColor;

    use super::{
        Modes, MouseMode, OscPolicy, ScreenModel, ScrollbackDumpPolicy, UnderlineStyle, WidthPolicy,
    };
    use crate::parser::ParseEvent;

    fn model() -> ScreenModel {
        ScreenModel::new(7, 4, 6)
    }

    fn text(s: &str) -> ParseEvent {
        ParseEvent::Text(s.to_owned())
    }

    fn sgr(params: &[u16]) -> ParseEvent {
        ParseEvent::Sgr {
            params: params.iter().map(|p| vec![*p]).collect(),
        }
    }

    fn sgr_groups(groups: &[&[u16]]) -> ParseEvent {
        ParseEvent::Sgr {
            params: groups.iter().map(|g| g.to_vec()).collect(),
        }
    }

    fn set_mode(code: u16, enabled: bool) -> ParseEvent {
        ParseEvent::SetMode {
            private_mode: true,
            code,
            enabled,
        }
    }

    fn scroll_region(top: u16, bottom: u16) -> ParseEvent {
        ParseEvent::SetScrollRegion { top, bottom }
    }

    fn char_at(model: &ScreenModel, row: usize, col: usize) -> String {
        model.snapshot().rows[row].cells[col]
            .text
            .clone()
            .unwrap_or_default()
    }

    #[test]
    fn text_writes_and_wraps_at_right_edge() {
        let mut model = model();
        model.apply_event(&text("abcdefgh"));
        // "abcdef" fills row 0; "g" wraps to row 1 col 0, "h" col 1.
        assert_eq!(char_at(&model, 0, 5), "f");
        assert_eq!(char_at(&model, 1, 0), "g");
        assert_eq!(char_at(&model, 1, 1), "h");
        assert_eq!(model.snapshot().cursor.row, 1);
        assert_eq!(model.snapshot().cursor.col, 2);
    }

    #[test]
    fn autowrap_off_keeps_cursor_at_right_edge() {
        let mut model = model();
        model.apply_event(&set_mode(7, false));
        model.apply_event(&text("abcdefghi"));
        // No wrap: all text stays on row 0, cursor pinned at the last column.
        assert_eq!(char_at(&model, 0, 5), "i");
        assert_eq!(char_at(&model, 1, 0), "");
        assert_eq!(model.snapshot().cursor.row, 0);
        assert_eq!(model.snapshot().cursor.col, 5);
    }

    #[test]
    fn linefeed_scrolls_within_region() {
        let mut model = model();
        model.apply_event(&scroll_region(1, 3)); // 1-based rows 1..=3 -> 0-based 0..=2
        assert_eq!(model.active_buffer().scroll_top(), 0);
        assert_eq!(model.active_buffer().scroll_bottom(), 2);
        // Fill the region then linefeed at the bottom scrolls.
        for _ in 0..3 {
            model.apply_event(&text("X"));
            model.apply_event(&ParseEvent::LineFeed);
        }
        // Row 0 was scrolled away; rows 0..=2 contain blanks/X from the last two writes.
        let snapshot = model.snapshot();
        assert_eq!(snapshot.cursor.row, 2);
        assert!(
            !snapshot.rows[3].cells.iter().any(|c| c.text.is_some()),
            "outside region must stay blank"
        );
    }

    #[test]
    fn origin_mode_positions_cursor_in_region() {
        let mut model = model();
        model.apply_event(&scroll_region(2, 5));
        model.apply_event(&set_mode(6, true));
        model.apply_event(&ParseEvent::CursorPosition { row: 1, col: 1 });
        // Origin mode: 1;1 maps to the scroll-region top (0-based row 1).
        assert_eq!(model.snapshot().cursor.row, 1);
        assert_eq!(model.snapshot().cursor.col, 0);
    }

    #[test]
    fn erase_display_and_line() {
        let mut model = model();
        model.apply_event(&text("hello"));
        model.apply_event(&ParseEvent::CursorPosition { row: 1, col: 1 });
        model.apply_event(&ParseEvent::EraseDisplay { mode: 2 });
        let snapshot = model.snapshot();
        assert!(snapshot
            .rows
            .iter()
            .all(|row| row.cells.iter().all(|c| c.text.is_none())));
        // Erase line mode 0 from the cursor (row 0, col 1).
        model.apply_event(&text("abc"));
        model.apply_event(&ParseEvent::CursorPosition { row: 1, col: 2 });
        model.apply_event(&ParseEvent::EraseLine { mode: 0 });
        assert_eq!(char_at(&model, 0, 0), "a");
        assert_eq!(char_at(&model, 0, 1), "");
        assert_eq!(char_at(&model, 0, 2), "");
    }

    #[test]
    fn sgr_basic_applies_style_to_cells() {
        let mut model = model();
        model.apply_event(&sgr(&[1, 31]));
        model.apply_event(&text("x"));
        let cell = &model.snapshot().rows[0].cells[0];
        assert!(cell.style.bold);
        assert_eq!(cell.style.fg, TerminalColor::Indexed(1));
        // Reset clears.
        model.apply_event(&sgr(&[0]));
        model.apply_event(&text("y"));
        let cell = &model.snapshot().rows[0].cells[1];
        assert!(!cell.style.bold);
        assert_eq!(cell.style.fg, TerminalColor::Default);
    }

    #[test]
    fn alternate_screen_switch_preserves_primary() {
        let mut model = model();
        model.apply_event(&text("P"));
        model.apply_event(&set_mode(1049, true)); // enter alternate, save cursor
        assert!(model.active_is_alternate());
        model.apply_event(&text("A"));
        assert_eq!(char_at(&model, 0, 0), "A");
        model.apply_event(&set_mode(1049, false)); // exit, restore primary
        assert!(!model.active_is_alternate());
        assert_eq!(char_at(&model, 0, 0), "P", "primary content preserved");
        assert_eq!(model.snapshot().cursor.row, 0);
        assert_eq!(model.snapshot().cursor.col, 1);
    }

    #[test]
    fn cursor_move_respects_scroll_region() {
        let mut model = model();
        model.apply_event(&ParseEvent::CursorMove {
            row_delta: 5,
            col_delta: 0,
        });
        // Moved past the bottom: scrolls the region and stays at the bottom.
        assert_eq!(model.snapshot().cursor.row, 3);
        // Move up above the top clamps to 0.
        model.apply_event(&ParseEvent::CursorMove {
            row_delta: -10,
            col_delta: 0,
        });
        assert_eq!(model.snapshot().cursor.row, 0);
    }

    #[test]
    fn title_and_modes_are_recorded() {
        let mut model = model();
        model.apply_event(&ParseEvent::Title("demo".to_owned()));
        assert_eq!(model.title(), Some("demo"));
        model.apply_event(&set_mode(2004, true));
        assert!(model.modes().bracketed_paste);
        model.apply_event(&set_mode(25, false));
        assert!(!model.snapshot().cursor.visible);
    }

    #[test]
    fn sgr_256_color_and_truecolor() {
        let mut model = model();
        model.apply_event(&sgr_groups(&[&[38], &[5], &[200]]));
        model.apply_event(&text("a"));
        let cell = &model.snapshot().rows[0].cells[0];
        assert_eq!(cell.style.fg, TerminalColor::Indexed(200));
        model.apply_event(&sgr_groups(&[&[48], &[5], &[21]]));
        model.apply_event(&text("b"));
        assert_eq!(
            model.snapshot().rows[0].cells[1].style.bg,
            TerminalColor::Indexed(21)
        );
        model.apply_event(&sgr_groups(&[&[38], &[2], &[255], &[128], &[0]]));
        model.apply_event(&text("c"));
        assert_eq!(
            model.snapshot().rows[0].cells[2].style.fg,
            TerminalColor::TrueColor {
                r: 255,
                g: 128,
                b: 0
            }
        );
    }

    #[test]
    fn sgr_bright_16_colors() {
        let mut model = model();
        model.apply_event(&sgr(&[91, 101]));
        model.apply_event(&text("x"));
        let cell = &model.snapshot().rows[0].cells[0];
        assert_eq!(cell.style.fg, TerminalColor::Indexed(9));
        assert_eq!(cell.style.bg, TerminalColor::Indexed(9));
    }

    #[test]
    fn sgr_inverse_dim_combination() {
        let mut model = model();
        model.apply_event(&sgr(&[2, 7]));
        model.apply_event(&text("x"));
        let cell = &model.snapshot().rows[0].cells[0];
        assert!(cell.style.dim);
        assert!(cell.style.inverse);
        // Reset clears both.
        model.apply_event(&sgr(&[0]));
        model.apply_event(&text("y"));
        let cell = &model.snapshot().rows[0].cells[1];
        assert!(!cell.style.dim);
        assert!(!cell.style.inverse);
    }

    #[test]
    fn sgr_underline_styles() {
        let mut model = model();
        model.apply_event(&sgr(&[4]));
        model.apply_event(&text("a"));
        let cell = &model.snapshot().rows[0].cells[0];
        assert!(cell.style.underline);
        assert_eq!(cell.style.underline_style, UnderlineStyle::Single);
        // 4:2 (colon sub-parameter) = double underline.
        model.apply_event(&sgr_groups(&[&[4, 2]]));
        model.apply_event(&text("b"));
        assert_eq!(
            model.snapshot().rows[0].cells[1].style.underline_style,
            UnderlineStyle::Double
        );
        // 21 = double underline.
        model.apply_event(&sgr(&[21]));
        model.apply_event(&text("c"));
        assert_eq!(
            model.snapshot().rows[0].cells[2].style.underline_style,
            UnderlineStyle::Double
        );
        // 4:3 curly, 4:5 dashed.
        model.apply_event(&sgr_groups(&[&[4, 3]]));
        model.apply_event(&text("d"));
        assert_eq!(
            model.snapshot().rows[0].cells[3].style.underline_style,
            UnderlineStyle::Curly
        );
        model.apply_event(&sgr_groups(&[&[4, 5]]));
        model.apply_event(&text("e"));
        assert_eq!(
            model.snapshot().rows[0].cells[4].style.underline_style,
            UnderlineStyle::Dashed
        );
        // 24 resets.
        model.apply_event(&sgr(&[24]));
        model.apply_event(&text("f"));
        let cell = &model.snapshot().rows[0].cells[5];
        assert!(!cell.style.underline);
        assert_eq!(cell.style.underline_style, UnderlineStyle::None);
    }

    #[test]
    fn osc_title_and_working_directory() {
        let mut model = model();
        model.apply_event(&ParseEvent::Title("demo".to_owned()));
        assert_eq!(model.title(), Some("demo"));
        model.apply_event(&ParseEvent::WorkingDirectory("/home/user".to_owned()));
        assert_eq!(model.working_directory(), Some("/home/user"));
        assert_eq!(
            model.snapshot().working_directory.as_deref(),
            Some("/home/user")
        );
    }

    #[test]
    fn osc_notification_policy_gating() {
        let mut model = model();
        // Denied by default: no notification surfaces from untrusted output.
        model.apply_event(&ParseEvent::Notification {
            summary: "spam".to_owned(),
            body: "click".to_owned(),
        });
        assert!(model.notification().is_none());
        // Opt in explicitly: notification stored; take() consumes it.
        let policy = OscPolicy {
            allow_notifications: true,
            ..OscPolicy::default()
        };
        model.set_osc_policy(policy);
        model.apply_event(&ParseEvent::Notification {
            summary: "build ok".to_owned(),
            body: "0 errors".to_owned(),
        });
        let notification = model.take_notification().expect("notification");
        assert_eq!(notification.summary, "build ok");
        assert_eq!(notification.body, "0 errors");
        assert!(model.notification().is_none());
    }

    #[test]
    fn osc_untitled_sequences_cannot_bypass_policy() {
        let mut model = model();
        // Embedded control bytes are stripped, not interpreted.
        model.apply_event(&ParseEvent::Title("a\x1b]0;b\x07c".to_owned()));
        assert_eq!(model.title(), Some("a]0;bc"));
        // Denying titles leaves the previous title intact.
        let policy = OscPolicy {
            allow_title: false,
            ..OscPolicy::default()
        };
        model.set_osc_policy(policy);
        model.apply_event(&ParseEvent::Title("evil".to_owned()));
        assert_eq!(model.title(), Some("a]0;bc"));
    }

    #[test]
    fn osc8_hyperlink_attaches_and_clears() {
        let mut model = model();
        model.apply_event(&ParseEvent::Hyperlink {
            id: Some("l1".to_owned()),
            url: "https://example.com/path".to_owned(),
        });
        model.apply_event(&text("ab"));
        let row = &model.snapshot().rows[0].cells;
        assert_eq!(
            row[0].hyperlink.as_ref().map(|h| h.url.as_str()),
            Some("https://example.com/path")
        );
        assert_eq!(
            row[0].hyperlink.as_ref().and_then(|h| h.id.as_deref()),
            Some("l1")
        );
        assert!(row[1].hyperlink.is_some());
        // Empty URI ends the hyperlink.
        model.apply_event(&ParseEvent::Hyperlink {
            id: None,
            url: String::new(),
        });
        model.apply_event(&text("c"));
        assert!(model.snapshot().rows[0].cells[2].hyperlink.is_none());
    }

    #[test]
    fn osc8_dangerous_scheme_is_ignored() {
        let mut model = model();
        model.apply_event(&ParseEvent::Hyperlink {
            id: None,
            url: "javascript:alert(1)".to_owned(),
        });
        model.apply_event(&text("x"));
        assert!(model.snapshot().rows[0].cells[0].hyperlink.is_none());
    }

    #[test]
    fn input_protocol_modes_wire() {
        let mut model = model();
        model.apply_event(&set_mode(1004, true));
        model.apply_event(&set_mode(1000, true));
        model.apply_event(&set_mode(1006, true));
        assert!(model.modes().focus_events);
        assert_eq!(model.modes().mouse_mode, MouseMode::Buttons);
        assert!(model.modes().mouse_sgr);
        model.apply_event(&set_mode(1003, true));
        assert_eq!(model.modes().mouse_mode, MouseMode::Motion);
        model.apply_event(&set_mode(1003, false));
        assert_eq!(model.modes().mouse_mode, MouseMode::Off);
    }

    #[test]
    fn scrollback_captures_scrolled_lines() {
        let mut model = ScreenModel::new(7, 3, 4);
        for i in 0..6u32 {
            model.apply_event(&ParseEvent::CarriageReturn);
            model.apply_event(&text(&format!("L{i}")));
            model.apply_event(&ParseEvent::LineFeed);
        }
        // LFs at the bottom scroll 4 lines into the ring buffer.
        assert_eq!(model.scrollback_len(), 4);
        assert_eq!(model.snapshot().scrollback_start, 4);
        // "L0" occupies cells[0..2]; the retained window is L0..=L3.
        assert_eq!(
            model.scrollback().get(0).unwrap().cells[0].text.as_deref(),
            Some("L")
        );
        assert_eq!(
            model.scrollback().get(0).unwrap().cells[1].text.as_deref(),
            Some("0")
        );
        assert_eq!(
            model.scrollback().get(3).unwrap().cells[1].text.as_deref(),
            Some("3")
        );
    }

    #[test]
    fn alternate_screen_has_no_scrollback() {
        let mut model = ScreenModel::new(7, 3, 4);
        model.apply_event(&set_mode(1049, true));
        for _ in 0..5 {
            model.apply_event(&text("x"));
            model.apply_event(&ParseEvent::LineFeed);
        }
        assert_eq!(model.scrollback_len(), 0);
    }

    #[test]
    fn scrollback_dump_off_by_default() {
        let mut model = ScreenModel::new(7, 3, 4);
        for _ in 0..5 {
            model.apply_event(&text("x"));
            model.apply_event(&ParseEvent::LineFeed);
        }
        assert!(model.scrollback_len() > 0);
        assert!(
            model.dump_scrollback(4).is_none(),
            "sensitive scrollback dumps must be off by default"
        );
        model.set_scrollback_dump_policy(ScrollbackDumpPolicy {
            allow_dump: true,
            max_bytes: 1024,
        });
        assert!(model.dump_scrollback(4).is_some());
    }

    #[test]
    fn modes_defaults() {
        let modes = Modes::default();
        assert!(modes.autowrap);
        assert!(!modes.insert);
        assert!(!modes.origin);
        assert!(modes.cursor_visible);
    }

    #[test]
    fn wide_char_occupies_two_columns() {
        let mut model = model();
        model.apply_event(&text("a\u{4e2d}b"));
        // "a" at col 0; "\u{4e2d}" (width 2) at col 1-2; "b" at col 3.
        assert_eq!(char_at(&model, 0, 0), "a");
        assert_eq!(char_at(&model, 0, 1), "\u{4e2d}");
        assert!(model.snapshot().rows[0].cells[2].wide_continuation);
        assert!(model.snapshot().rows[0].cells[2].text.is_none());
        assert_eq!(char_at(&model, 0, 3), "b");
        assert_eq!(model.snapshot().cursor.col, 4);
    }

    #[test]
    fn combining_sequence_is_one_cell() {
        let mut model = model();
        model.apply_event(&text("e\u{301}x"));
        // e + combining acute is one grapheme cluster in one cell.
        assert_eq!(char_at(&model, 0, 0), "e\u{301}");
        assert_eq!(char_at(&model, 0, 1), "x");
        assert_eq!(model.snapshot().cursor.col, 2);
    }

    #[test]
    fn emoji_zwj_is_one_wide_cell() {
        let mut model = model();
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        model.apply_event(&text(family));
        assert_eq!(char_at(&model, 0, 0), family);
        assert!(model.snapshot().rows[0].cells[1].wide_continuation);
        assert_eq!(model.snapshot().cursor.col, 2);
    }

    #[test]
    fn overwrite_breaks_wide_pair() {
        let mut model = model();
        model.apply_event(&text("\u{4e2d}"));
        model.apply_event(&ParseEvent::CursorPosition { row: 1, col: 2 });
        model.apply_event(&text("x"));
        // The continuation cell was overwritten, so the base is cleared too.
        assert_eq!(char_at(&model, 0, 0), "");
        assert_eq!(char_at(&model, 0, 1), "x");
        assert!(!model.snapshot().rows[0].cells[1].wide_continuation);
    }

    #[test]
    fn erase_continuation_breaks_wide_pair() {
        let mut model = model();
        model.apply_event(&text("\u{4e2d}"));
        model.apply_event(&ParseEvent::CursorPosition { row: 1, col: 2 });
        model.apply_event(&ParseEvent::EraseLine { mode: 0 });
        assert_eq!(char_at(&model, 0, 0), "");
        assert!(!model.snapshot().rows[0].cells[1].wide_continuation);
    }

    #[test]
    fn width_policy_is_configurable() {
        // Default: Unicode (ambiguous = 1).
        let mut unicode_model = model();
        unicode_model.apply_event(&text("\u{00b7}"));
        assert_eq!(unicode_model.snapshot().cursor.col, 1);
        // East Asian: ambiguous = 2 with a marked continuation.
        let mut ea_model = model();
        ea_model.set_width_policy(WidthPolicy::EastAsian);
        ea_model.apply_event(&text("\u{00b7}"));
        assert_eq!(ea_model.snapshot().cursor.col, 2);
        assert!(ea_model.snapshot().rows[0].cells[1].wide_continuation);
        // Legacy: CJK is 1 column.
        let mut legacy_model = model();
        legacy_model.set_width_policy(WidthPolicy::Legacy);
        legacy_model.apply_event(&text("\u{4e2d}"));
        assert_eq!(legacy_model.snapshot().cursor.col, 1);
        assert!(!legacy_model.snapshot().rows[0].cells[1].wide_continuation);
    }
}
