//! Pure, headless-testable model for the Windows PC GUI.
//!
//! This module is the entire UI logic of the PC frontend and contains no
//! Win32 calls: a fixed-size terminal grid with vertical scroll, an input
//! line, a command queue, and session-phase state. The native shell in
//! `main.rs` only renders this model and feeds it keystrokes, so the whole
//! GUI can be verified deterministically without opening a window.

use std::collections::VecDeque;

use abi_c::{BatchItem, EventBatch};

/// Default terminal rows.
pub const DEFAULT_ROWS: usize = 24;
/// Default terminal columns.
pub const DEFAULT_COLS: usize = 80;
/// Maximum input-line length in characters.
pub const MAX_INPUT_LEN: usize = 1024;

/// A 24-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl Rgb {
    /// Creates a new RGB color.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Default terminal foreground (light gray).
pub const DEFAULT_FG: Rgb = Rgb::new(0xE6, 0xE6, 0xE6);
/// Default terminal background (near black).
pub const DEFAULT_BG: Rgb = Rgb::new(0x0C, 0x0C, 0x0C);
/// Accent color used for the prompt, status bar, and caret.
pub const ACCENT_FG: Rgb = Rgb::new(0x4F, 0xC3, 0xF7);
/// Error color used for unknown commands.
pub const ERROR_FG: Rgb = Rgb::new(0xEF, 0x53, 0x50);
/// Input-area background color.
pub const INPUT_BG: Rgb = Rgb::new(0x1E, 0x1E, 0x1E);

/// One terminal cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The glyph.
    pub ch: char,
    /// Foreground color.
    pub fg: Rgb,
    /// Background color.
    pub bg: Rgb,
}

impl Cell {
    /// A blank cell (space on the default palette).
    pub const fn blank() -> Self {
        Self {
            ch: ' ',
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
        }
    }

    /// A cell with an explicit glyph and colors.
    pub const fn new(ch: char, fg: Rgb, bg: Rgb) -> Self {
        Self { ch, fg, bg }
    }
}

/// A fixed-size terminal grid with vertical scroll.
///
/// Writing past the last row scrolls the viewport up by one line (the oldest
/// line is discarded). The grid is the single source of truth the native
/// shell renders; tests assert on [`Grid::to_lines`] and [`Grid::row_runs`].
#[derive(Debug, Clone)]
pub struct Grid {
    rows: usize,
    cols: usize,
    cells: Vec<Cell>,
    row: usize,
    col: usize,
    scrolled_lines: u64,
}

impl Grid {
    /// Creates a blank grid.
    pub fn new(rows: usize, cols: usize) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        Self {
            rows,
            cols,
            cells: vec![Cell::blank(); rows * cols],
            row: 0,
            col: 0,
            scrolled_lines: 0,
        }
    }

    /// Visible row count.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Visible column count.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Lines scrolled off the top since construction or the last clear.
    pub fn scrolled_lines(&self) -> u64 {
        self.scrolled_lines
    }

    /// The cell at a viewport position (blank when out of bounds).
    pub fn cell(&self, row: usize, col: usize) -> Cell {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col]
        } else {
            Cell::blank()
        }
    }

    /// The cursor position as `(row, col)`.
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// Appends text at the cursor, honoring `\n`, `\r`, and tab stops.
    pub fn write_text(&mut self, text: &str) {
        for ch in text.chars() {
            match ch {
                '\n' => self.newline(),
                '\r' => self.col = 0,
                '\t' => {
                    let next_tab = (self.col / 8 + 1) * 8;
                    let target = next_tab.min(self.cols);
                    while self.col < target {
                        self.put_char(' ');
                    }
                }
                c if c.is_control() => {
                    // Remaining control codes are handled by the full parser
                    // in terminal-parser; the host shell ignores them here.
                }
                c => self.put_char(c),
            }
        }
    }

    /// Writes text using an explicit foreground color (used for errors).
    pub fn write_colored(&mut self, text: &str, fg: Rgb) {
        for ch in text.chars() {
            match ch {
                '\n' => self.newline(),
                '\r' => self.col = 0,
                c if c.is_control() => {}
                c => self.put_char_colored(c, fg),
            }
        }
    }

    /// Moves to the start of the next line, scrolling when at the bottom.
    pub fn newline(&mut self) {
        if self.row + 1 >= self.rows {
            self.scroll_up();
        } else {
            self.row += 1;
        }
        self.col = 0;
    }

    /// Clears the grid and resets the cursor and scroll counter.
    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            *cell = Cell::blank();
        }
        self.row = 0;
        self.col = 0;
        self.scrolled_lines = 0;
    }

    /// Resizes the viewport, keeping the bottom rows of existing content.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        let keep = self.rows.min(rows);
        let mut next = vec![Cell::blank(); rows * cols];
        for row in 0..keep {
            let src = self.rows - keep + row;
            for col in 0..self.cols.min(cols) {
                next[row * cols + col] = self.cells[src * self.cols + col];
            }
        }
        self.rows = rows;
        self.cols = cols;
        self.cells = next;
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
    }

    /// Renders each visible row as a string (for tests and the shell).
    pub fn to_lines(&self) -> Vec<String> {
        (0..self.rows)
            .map(|row| {
                let mut line = String::with_capacity(self.cols);
                for col in 0..self.cols {
                    line.push(self.cell(row, col).ch);
                }
                line
            })
            .collect()
    }

    /// Renders one visible row as foreground-colored runs so the native shell
    /// can draw text without re-deriving the model.
    pub fn row_runs(&self, row: usize) -> Vec<(Rgb, String)> {
        let mut runs: Vec<(Rgb, String)> = Vec::new();
        for col in 0..self.cols {
            let cell = self.cell(row, col);
            match runs.last_mut() {
                Some((fg, text)) if *fg == cell.fg => text.push(cell.ch),
                _ => runs.push((cell.fg, cell.ch.to_string())),
            }
        }
        runs
    }

    fn put_char(&mut self, ch: char) {
        self.put_char_colored(ch, DEFAULT_FG);
    }

    fn put_char_colored(&mut self, ch: char, fg: Rgb) {
        if self.col >= self.cols {
            self.newline();
        }
        self.cells[self.row * self.cols + self.col] = Cell::new(ch, fg, DEFAULT_BG);
        self.col += 1;
        if self.col >= self.cols {
            self.newline();
        }
    }

    fn scroll_up(&mut self) {
        self.cells.copy_within(self.cols.., 0);
        for cell in self.cells.iter_mut().skip((self.rows - 1) * self.cols) {
            *cell = Cell::blank();
        }
        self.scrolled_lines += 1;
    }
}

/// Session phase shown in the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    /// No session.
    Disconnected,
    /// A connect was requested and the transport is being established.
    Connecting,
    /// The transport reported the session is up.
    Connected,
    /// The session is tearing down.
    Closing,
}

impl SessionPhase {
    /// A stable machine-readable label.
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionPhase::Disconnected => "disconnected",
            SessionPhase::Connecting => "connecting",
            SessionPhase::Connected => "connected",
            SessionPhase::Closing => "closing",
        }
    }
}

/// Commands the native shell must act on (everything else is model state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiCommand {
    /// Close the GUI (the window procedure posts WM_QUIT).
    Quit,
    /// A plain input line destined for the transport (via the abi-c bridge).
    SendLine(String),
}

/// Help text shown for `/help`.
pub const HELP_TEXT: &str =
    "commands: /connect [user@]host | /disconnect | /clear | /help | /quit\n";

/// The complete, deterministic UI model of the PC GUI.
#[derive(Debug, Clone)]
pub struct GuiModel {
    grid: Grid,
    phase: SessionPhase,
    status: String,
    input: String,
    input_cursor: usize,
    host: String,
    user: String,
    commands: VecDeque<GuiCommand>,
    needs_snapshot: bool,
}

impl GuiModel {
    /// A model with the default 80x24 grid.
    pub fn new() -> Self {
        Self::with_size(DEFAULT_ROWS, DEFAULT_COLS)
    }

    /// A model with an explicit grid size.
    pub fn with_size(rows: usize, cols: usize) -> Self {
        Self {
            grid: Grid::new(rows, cols),
            phase: SessionPhase::Disconnected,
            status: "disconnected — type /help for commands".to_string(),
            input: String::new(),
            input_cursor: 0,
            host: String::new(),
            user: String::new(),
            commands: VecDeque::new(),
            needs_snapshot: false,
        }
    }

    /// The terminal grid.
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// The current session phase.
    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    /// The current status text.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The current input line.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// The input cursor position in characters.
    pub fn input_cursor(&self) -> usize {
        self.input_cursor
    }

    /// The target host (empty until `/connect`).
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The target user (empty until `/connect user@host`).
    pub fn user(&self) -> &str {
        &self.user
    }

    /// Whether the consumer must request a snapshot after dropped events.
    pub fn needs_snapshot(&self) -> bool {
        self.needs_snapshot
    }

    /// A single-line status description for the status bar.
    pub fn status_line(&self) -> String {
        format!(
            "{} | {} | scrolled {} lines",
            self.phase.as_str(),
            self.status,
            self.grid.scrolled_lines()
        )
    }

    /// The input line prefixed by the prompt glyph.
    pub fn input_line(&self) -> String {
        format!("> {}", self.input)
    }

    /// Resizes the model viewport.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.grid.resize(rows, cols);
    }

    /// Appends transport output to the terminal grid.
    pub fn append_output(&mut self, text: &str) {
        self.grid.write_text(text);
    }

    /// Appends error-colored output (unknown commands, usage errors).
    pub fn append_error(&mut self, text: &str) {
        self.grid.write_colored(text, ERROR_FG);
    }

    /// Inserts a printable character at the cursor.
    pub fn type_char(&mut self, ch: char) {
        if ch.is_control() || self.input.chars().count() >= MAX_INPUT_LEN {
            return;
        }
        let index = self.byte_index(self.input_cursor);
        self.input.insert(index, ch);
        self.input_cursor += 1;
    }

    /// Removes the character before the cursor.
    pub fn backspace(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let index = self.byte_index(self.input_cursor - 1);
        self.input.remove(index);
        self.input_cursor -= 1;
    }

    /// Removes the character at the cursor.
    pub fn delete_forward(&mut self) {
        let len = self.input.chars().count();
        if self.input_cursor >= len {
            return;
        }
        let index = self.byte_index(self.input_cursor);
        self.input.remove(index);
    }

    /// Moves the cursor one character left.
    pub fn cursor_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
    }

    /// Moves the cursor one character right.
    pub fn cursor_right(&mut self) {
        if self.input_cursor < self.input.chars().count() {
            self.input_cursor += 1;
        }
    }

    /// Clears the input line.
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.input_cursor = 0;
    }

    /// Submits the current input line: echoes it, then applies or queues it.
    pub fn submit(&mut self) {
        let line = self.input.trim().to_string();
        self.clear_input();
        if line.is_empty() {
            return;
        }
        self.append_output(&format!("{line}\n"));
        if let Some(slash) = line.strip_prefix('/') {
            self.apply_slash_command(slash);
        } else {
            self.commands.push_back(GuiCommand::SendLine(line));
        }
    }

    /// Starts a connection to `[user@]host` (phase becomes connecting).
    pub fn start_connect(&mut self, target: &str) {
        let (user, host) = match target.split_once('@') {
            Some((user, host)) => (user.to_string(), host.to_string()),
            None => (String::new(), target.to_string()),
        };
        if host.is_empty() {
            self.append_error("usage: /connect [user@]host\n");
            return;
        }
        self.user = user;
        self.host = host;
        self.phase = SessionPhase::Connecting;
        let who = if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        };
        self.status =
            format!("connecting to {who} (host shell; transport via abi-c not yet wired)");
        self.append_output(&format!("connecting to {who} ...\n"));
    }

    /// Marks the session connected.
    pub fn on_connected(&mut self) {
        self.phase = SessionPhase::Connected;
        self.status = "connected".to_string();
        self.append_output("connected\n");
    }

    /// Returns to the disconnected state with a reason.
    pub fn disconnect(&mut self, reason: &str) {
        self.phase = SessionPhase::Disconnected;
        self.status = "disconnected".to_string();
        self.append_output(&format!("disconnected: {reason}\n"));
    }

    /// Takes the next command the shell must act on.
    pub fn pop_command(&mut self) -> Option<GuiCommand> {
        self.commands.pop_front()
    }

    /// Applies one abi-c event batch to the model (the ABI transfer unit).
    pub fn apply_batch(&mut self, batch: &EventBatch) {
        for item in &batch.items {
            match item {
                BatchItem::Event(bytes) => {
                    if let Ok(text) = std::str::from_utf8(bytes) {
                        self.append_output(text);
                    }
                }
                BatchItem::SnapshotRequired => {
                    self.needs_snapshot = true;
                }
                BatchItem::Snapshot(data) => {
                    self.needs_snapshot = false;
                    if let Ok(text) = std::str::from_utf8(data) {
                        self.grid.clear();
                        self.append_output(text);
                    }
                }
            }
        }
    }

    fn apply_slash_command(&mut self, slash: &str) {
        let mut parts = slash.split_whitespace();
        let command = parts.next().unwrap_or_default();
        let argument = parts.collect::<Vec<_>>().join(" ");
        match command {
            "connect" => self.start_connect(&argument),
            "disconnect" => self.disconnect("user requested"),
            "clear" => self.grid.clear(),
            "help" => self.append_output(HELP_TEXT),
            "quit" | "exit" => self.commands.push_back(GuiCommand::Quit),
            _ => self.append_error(&format!("unknown command: /{command} (try /help)\n")),
        }
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_index)
            .map(|(index, _)| index)
            .unwrap_or(self.input.len())
    }
}

impl Default for GuiModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(items: Vec<BatchItem>) -> EventBatch {
        EventBatch {
            version: abi_c::EVENT_BATCH_VERSION,
            sequence: 1,
            items,
            total_bytes: 0,
            dropped: 0,
        }
    }

    #[test]
    fn grid_wraps_long_lines() {
        let mut grid = Grid::new(2, 4);
        grid.write_text("abcdef");
        let lines = grid.to_lines();
        assert_eq!(lines[0], "abcd");
        assert_eq!(lines[1], "ef  ");
        assert_eq!(grid.cursor(), (1, 2));
    }

    #[test]
    fn grid_scrolls_when_full() {
        let mut grid = Grid::new(2, 4);
        grid.write_text("one\ntwo\nthree");
        let lines = grid.to_lines();
        // "one" scrolls off; "two" scrolls off when "three" wraps the row;
        // the wrapped word fills the top row and its trailing "e" lands on
        // the fresh last row.
        assert_eq!(lines[0], "thre");
        assert_eq!(lines[1].trim_end(), "e");
        assert_eq!(grid.scrolled_lines(), 2);
    }

    #[test]
    fn grid_clear_and_resize_keep_bottom() {
        let mut grid = Grid::new(4, 4);
        grid.write_text("1\n2\n3\n4");
        grid.resize(2, 6);
        let lines = grid.to_lines();
        assert_eq!(lines[0].trim_end(), "3");
        assert_eq!(lines[1].trim_end(), "4");
        grid.clear();
        assert!(grid.to_lines().iter().all(|line| line.trim().is_empty()));
        assert_eq!(grid.scrolled_lines(), 0);
    }

    #[test]
    fn grid_renders_color_runs() {
        let mut grid = Grid::new(2, 5);
        grid.write_text("abc");
        grid.write_colored("!?", ERROR_FG);
        let runs = grid.row_runs(0);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].0, DEFAULT_FG);
        assert_eq!(runs[0].1, "abc");
        assert_eq!(runs[1].0, ERROR_FG);
        assert_eq!(runs[1].1, "!?");
    }

    #[test]
    fn input_insert_backspace_and_cursor() {
        let mut model = GuiModel::with_size(2, 8);
        model.type_char('h');
        model.type_char('i');
        model.cursor_left();
        model.type_char('X');
        assert_eq!(model.input(), "hXi");
        assert_eq!(model.input_cursor(), 2);
        model.backspace();
        assert_eq!(model.input(), "hi");
        assert_eq!(model.input_cursor(), 1);
        model.delete_forward();
        assert_eq!(model.input(), "h");
        model.clear_input();
        assert!(model.input().is_empty());
        assert_eq!(model.input_cursor(), 0);
    }

    #[test]
    fn control_chars_are_ignored_in_input() {
        let mut model = GuiModel::with_size(2, 8);
        model.type_char('a');
        model.type_char('\u{1b}');
        model.type_char('b');
        assert_eq!(model.input(), "ab");
    }

    #[test]
    fn submit_echoes_and_queues_send_line() {
        let mut model = GuiModel::with_size(4, 16);
        model.type_char('h');
        model.type_char('i');
        model.submit();
        assert_eq!(
            model.pop_command(),
            Some(GuiCommand::SendLine("hi".to_string()))
        );
        assert_eq!(model.pop_command(), None);
        assert!(model.input().is_empty());
        assert!(model.grid().to_lines()[0].contains("hi"));
    }

    #[test]
    fn connect_parses_user_and_host_and_moves_phase() {
        let mut model = GuiModel::with_size(4, 32);
        for ch in "/connect demo@host".chars() {
            model.type_char(ch);
        }
        model.submit();
        assert_eq!(model.phase(), SessionPhase::Connecting);
        assert_eq!(model.user(), "demo");
        assert_eq!(model.host(), "host");
        assert_eq!(model.pop_command(), None);
        model.on_connected();
        assert_eq!(model.phase(), SessionPhase::Connected);
        model.disconnect("user requested");
        assert_eq!(model.phase(), SessionPhase::Disconnected);
        assert!(model.status().contains("disconnected"));
    }

    #[test]
    fn connect_without_host_reports_usage() {
        let mut model = GuiModel::with_size(4, 32);
        for ch in "/connect".chars() {
            model.type_char(ch);
        }
        model.submit();
        assert_eq!(model.phase(), SessionPhase::Disconnected);
        let cell = model.grid().cell(1, 0);
        assert_eq!(cell.fg, ERROR_FG);
        assert!(model.grid().to_lines()[1].contains("usage:"));
    }

    #[test]
    fn quit_is_queued_and_unknown_command_reports_error() {
        let mut model = GuiModel::with_size(4, 32);
        for ch in "/quit".chars() {
            model.type_char(ch);
        }
        model.submit();
        assert_eq!(model.pop_command(), Some(GuiCommand::Quit));
        for ch in "/frobnicate".chars() {
            model.type_char(ch);
        }
        model.submit();
        assert_eq!(model.pop_command(), None);
        assert!(model
            .grid()
            .to_lines()
            .iter()
            .any(|line| line.contains("unknown command")));
    }

    #[test]
    fn event_batch_appends_output_and_snapshot_recovers() {
        let mut model = GuiModel::with_size(2, 16);
        model.apply_batch(&batch(vec![BatchItem::Event(b"hello\n".to_vec())]));
        assert_eq!(model.grid().to_lines()[0], "hello           ");
        model.apply_batch(&batch(vec![BatchItem::SnapshotRequired]));
        assert!(model.needs_snapshot());
        model.apply_batch(&batch(vec![BatchItem::Snapshot(b"snap\n".to_vec())]));
        assert!(!model.needs_snapshot());
        assert_eq!(model.grid().to_lines()[0], "snap            ");
    }

    #[test]
    fn invalid_utf8_events_are_skipped() {
        let mut model = GuiModel::with_size(2, 16);
        model.apply_batch(&batch(vec![BatchItem::Event(vec![0xff, 0xfe, 0x00])]));
        assert!(model
            .grid()
            .to_lines()
            .iter()
            .all(|line| line.trim().is_empty()));
    }

    #[test]
    fn phase_labels_are_stable() {
        assert_eq!(SessionPhase::Disconnected.as_str(), "disconnected");
        assert_eq!(SessionPhase::Connecting.as_str(), "connecting");
        assert_eq!(SessionPhase::Connected.as_str(), "connected");
        assert_eq!(SessionPhase::Closing.as_str(), "closing");
    }
}
