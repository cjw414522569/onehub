//! T063 golden test: feed a vttest-basic-style script through the bounded
//! byte-stream parser (T062) into the screen model (T063) and compare the
//! resulting snapshot against a committed golden JSON. A change in DEC/ANSI
//! base-state behaviour fails the test with a golden diff.
//!
//! The full vttest interactive suite requires a real terminal and is
//! `blocked_environment` on CI hosts without one; this deterministic golden
//! script covers the DEC/ANSI base state (SGR, erase, cursor, scroll region,
//! origin mode, alternate screen, title).

use std::fs;

use terminal_parser::BoundedByteStreamParser;
use terminal_state::{ScreenModel, TerminalParser};

/// A vttest-basic-style script, plus T064 Unicode width/grapheme coverage:
/// a CJK wide character, a combining sequence, and a ZWJ family emoji.
fn script() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        b"\x1b[31;1mred bold\x1b[0m plain\r\n\
          \x1b[2J\x1b[2;3HX\x1b[4;1H\x1b[Ktail\r\n\
          \x1b[1;5r\x1b[?6h\x1b[1;1Ha\x1b[2;1Hb\x1b[3;1Hc\r\n\
          \x1b[?1049h\x1b[2Jalt-screen\x1b[?1049l\x1b]0;vttest-basic\x07\
          \x1b]7;file:///home/user/demo\x07",
    );
    bytes.extend_from_slice("\x1b[5;1H".as_bytes());
    bytes.extend_from_slice("\u{4e2d}".as_bytes()); // CJK wide char (width 2)
    bytes.extend_from_slice("e\u{301}".as_bytes()); // e + combining acute
    bytes.extend_from_slice(
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}".as_bytes(), // family emoji
    );
    // T067 OSC 8 hyperlink: disable origin mode, position to row 8, attach a
    // link to "click", then end it.
    bytes.extend_from_slice("\x1b[?6l\x1b[8;1H\x1b[0m".as_bytes());
    bytes.extend_from_slice("\x1b]8;id=doc;https://example.com/path\x07".as_bytes());
    bytes.extend_from_slice("click".as_bytes());
    bytes.extend_from_slice("\x1b]8;;\x07".as_bytes());
    bytes
}

/// A color-matrix script (T065): 16/256/truecolor, inverse/dim, underline
/// styles, and reset.
fn color_script() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x1b[2J");
    // 16-color foregrounds (0, 1, 7, 8, 15).
    bytes.extend_from_slice(b"\x1b[1;1H\x1b[0m\x1b[30m0\x1b[31m1\x1b[37m7\x1b[90m8\x1b[97mA");
    // 16-color backgrounds.
    bytes.extend_from_slice(b"\x1b[2;1H\x1b[0m\x1b[40m0\x1b[41m1\x1b[107mB");
    // 256-color fg/bg.
    bytes.extend_from_slice(b"\x1b[3;1H\x1b[0m\x1b[38;5;196mR\x1b[48;5;21mB");
    // Truecolor fg.
    bytes.extend_from_slice(b"\x1b[4;1H\x1b[0m\x1b[38;2;255;128;0mT");
    // Inverse + dim combination.
    bytes.extend_from_slice(b"\x1b[5;1H\x1b[0m\x1b[2;7mID");
    // Underline styles: single, double (4:2), curly (4:3).
    bytes.extend_from_slice(b"\x1b[6;1H\x1b[0m\x1b[4mS\x1b[4:2mD\x1b[4:3mC");
    // Reset returns to defaults.
    bytes.extend_from_slice(b"\x1b[7;1H\x1b[0m\x1b[1;31mX\x1b[0mN");
    bytes
}

#[test]
fn golden_color_matrix_snapshot() {
    let mut parser = BoundedByteStreamParser::new();
    let mut model = ScreenModel::new(7, 8, 20);
    let mut batch = parser.feed(&color_script());
    let tail = parser.finish();
    batch.events.extend(tail.events);
    batch.diagnostics.extend(tail.diagnostics);
    assert!(
        batch.diagnostics.is_empty(),
        "parser must not diagnose the color script"
    );
    model.apply_batch(&batch);
    let snapshot = model.snapshot();

    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/color-matrix.json"
    );
    let actual = serde_json::to_string_pretty(&snapshot).expect("serialize snapshot");
    match fs::read_to_string(golden_path) {
        Ok(expected) => {
            assert_eq!(
                actual.trim(),
                expected.trim(),
                "golden diff: regenerate with the golden-update test or fix the model"
            );
        }
        Err(_) => {
            fs::write(golden_path, actual).expect("write golden");
            panic!("golden file did not exist; wrote a fresh golden (first run)");
        }
    }
}

#[test]
fn golden_vttest_basic_snapshot() {
    let mut parser = BoundedByteStreamParser::new();
    let mut model = ScreenModel::new(7, 8, 20);
    let mut batch = parser.feed(&script());
    // finish() flushes coalesced end-of-stream text (the golden script ends
    // with a wide CJK char, a combining sequence, and a ZWJ emoji).
    let tail = parser.finish();
    batch.events.extend(tail.events);
    batch.diagnostics.extend(tail.diagnostics);
    assert!(
        batch.diagnostics.is_empty(),
        "parser must not diagnose the golden script"
    );
    model.apply_batch(&batch);
    let snapshot = model.snapshot();

    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/vttest-basic.json"
    );
    let actual = serde_json::to_string_pretty(&snapshot).expect("serialize snapshot");
    match fs::read_to_string(golden_path) {
        Ok(expected) => {
            assert_eq!(
                actual.trim(),
                expected.trim(),
                "golden diff: regenerate with the golden-update test or fix the model"
            );
        }
        Err(_) => {
            fs::write(golden_path, actual).expect("write golden");
            panic!("golden file did not exist; wrote a fresh golden (first run)");
        }
    }
}
