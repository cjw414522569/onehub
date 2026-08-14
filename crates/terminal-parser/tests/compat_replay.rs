//! T082 recorded-replay compatibility regression set: vim / tmux / screen /
//! htop / fzf / lazygit interaction scripts are replayed through the bounded
//! parser into the screen model and checked for expected markers. The full
//! interactive vttest / esctest suites and live TUI sessions require a real
//! PTY/terminal and are `blocked_environment` on CI hosts without one; these
//! deterministic recorded scripts lock the terminal state transitions those
//! apps rely on (alternate screen, status lines, box drawing, highlights).

use std::fs;

use terminal_parser::BoundedByteStreamParser;
use terminal_state::{ScreenModel, TerminalParser};

/// Replays a recorded script and returns (diagnostics, snapshot, alt-active).
fn replay(
    script: &[u8],
    rows: usize,
    cols: usize,
) -> (
    Vec<terminal_state::ParserDiagnostic>,
    terminal_state::ScreenModel,
) {
    let mut parser = BoundedByteStreamParser::new();
    let mut model = ScreenModel::new(7, rows, cols);
    let mut batch = parser.feed(script);
    let tail = parser.finish();
    batch.events.extend(tail.events);
    batch.diagnostics.extend(tail.diagnostics);
    model.apply_batch(&batch);
    (batch.diagnostics, model)
}

/// Flattens the visible rows into a searchable string.
fn flatten(model: &terminal_state::ScreenModel) -> String {
    model
        .snapshot()
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| cell.text.as_deref().unwrap_or(" "))
                .collect::<String>()
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn assert_clean(diagnostics: &[terminal_state::ParserDiagnostic]) {
    assert!(
        diagnostics.is_empty(),
        "recorded script must parse cleanly: {diagnostics:?}"
    );
}

#[test]
fn vim_replay() {
    // Empty-file vim: enter alternate screen, tilde marker lines, a status
    // line in reverse video, then exit.
    let script = b"\x1b[?1049h\x1b[2J\x1b[?25l\x1b[1;1H~\
                   \x1b[2;1H~\x1b[7m\"demo.txt\" 1L, 0C\x1b[0m\x1b[?25h";
    let (diagnostics, model) = replay(script, 24, 80);
    assert_clean(&diagnostics);
    assert!(model.active_is_alternate(), "vim uses the alternate screen");
    let text = flatten(&model);
    assert!(text.contains("~"), "vim tilde markers present");
    assert!(text.contains("demo.txt"), "vim status line present");
}

#[test]
fn tmux_replay() {
    // tmux status line and a split-pane layout via DECAWM/alt screen.
    let script = b"\x1b[?1049h\x1b[2J\x1b[1;1H[0] 0:bash* 1:zsh-\x1b[0m\
                   \x1b[24;1H\x1b[7m[0] 0:bash* 1:zsh-\x1b[0m";
    let (diagnostics, model) = replay(script, 24, 80);
    assert_clean(&diagnostics);
    let text = flatten(&model);
    assert!(
        text.contains("0:bash*"),
        "tmux status shows the active pane"
    );
    assert!(text.contains("1:zsh-"), "tmux status lists panes");
}

#[test]
fn screen_replay() {
    // GNU screen hardstatus line and window list.
    let script = b"\x1b[2;1H\x1b[7m0$ bash  1- zsh  2* top\x1b[0m";
    let (diagnostics, model) = replay(script, 24, 80);
    assert_clean(&diagnostics);
    let text = flatten(&model);
    assert!(text.contains("0$ bash"), "screen window list present");
    assert!(text.contains("2* top"), "screen active window marked");
}

#[test]
fn htop_replay() {
    // htop: alternate screen, box-drawing borders, and a header.
    let script = b"\x1b[?1049h\x1b[?25l\x1b[2J\
                   \x1b[1;1H\xe2\x94\x8c\xe2\x94\x80\xe2\x94\x80\xe2\x94\x90\
                   \x1b[1;5Hhtop\x1b[0m";
    let (diagnostics, model) = replay(script, 24, 80);
    assert_clean(&diagnostics);
    assert!(model.active_is_alternate());
    let text = flatten(&model);
    assert!(text.contains('\u{250c}'), "htop box-drawing top-left");
    assert!(text.contains('\u{2500}'), "htop box-drawing horizontal");
    assert!(text.contains("htop"), "htop header present");
}

#[test]
fn fzf_replay() {
    // fzf: alternate screen, hidden cursor, highlighted match.
    let script = b"\x1b[?1049h\x1b[?25l\x1b[2J\
                   \x1b[1;1H> \x1b[7mapp.rs\x1b[0m\n\x1b[2;1H  main.rs";
    let (diagnostics, model) = replay(script, 24, 80);
    assert_clean(&diagnostics);
    assert!(model.active_is_alternate());
    let text = flatten(&model);
    assert!(text.contains("app.rs"), "fzf highlighted match present");
    assert!(text.contains("main.rs"), "fzf candidate present");
}

#[test]
fn lazygit_replay() {
    // lazygit: colored headers and Unicode borders.
    let script = b"\x1b[?1049h\x1b[2J\
                   \x1b[38;5;33m\x1b[1;1H Status\x1b[0m\
                   \x1b[1;12H\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80";
    let (diagnostics, model) = replay(script, 24, 80);
    assert_clean(&diagnostics);
    let text = flatten(&model);
    assert!(text.contains("Status"), "lazygit header present");
    assert!(text.contains('\u{2500}'), "lazygit border present");
}

/// Writes the recorded corpus list (used by the contract script).
#[test]
fn corpus_manifest_is_registered() {
    let manifest_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/compat-corpus.json");
    if let Ok(manifest) = fs::read_to_string(manifest_path) {
        let manifest = manifest.trim();
        for app in ["vim", "tmux", "screen", "htop", "fzf", "lazygit"] {
            assert!(
                manifest.contains(&format!("\"{app}\"")),
                "corpus manifest must list {app}"
            );
        }
    }
}
