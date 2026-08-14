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

/// A vttest-basic-style script.
fn script() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        b"\x1b[31;1mred bold\x1b[0m plain\r\n\
          \x1b[2J\x1b[2;3HX\x1b[4;1H\x1b[Ktail\r\n\
          \x1b[1;5r\x1b[?6h\x1b[1;1Ha\x1b[2;1Hb\x1b[3;1Hc\r\n\
          \x1b[?1049h\x1b[2Jalt-screen\x1b[?1049l\x1b]0;vttest-basic\x07",
    );
    bytes
}

#[test]
fn golden_vttest_basic_snapshot() {
    let mut parser = BoundedByteStreamParser::new();
    let mut model = ScreenModel::new(7, 8, 20);
    let batch = parser.feed(&script());
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
