use serde::{Deserialize, Serialize};

/// Terminal color: default, indexed palette, or truecolor RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TerminalColor {
    /// Terminal default color.
    #[default]
    Default,
    /// Indexed palette color (0..=255).
    Indexed(u8),
    /// Truecolor RGB.
    TrueColor { r: u8, g: u8, b: u8 },
}

/// Underline style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum UnderlineStyle {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// Rendering style of a cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalStyle {
    /// Foreground color.
    #[serde(default)]
    pub fg: TerminalColor,
    /// Background color.
    #[serde(default)]
    pub bg: TerminalColor,
    /// Bold.
    #[serde(default)]
    pub bold: bool,
    /// Italic.
    #[serde(default)]
    pub italic: bool,
    /// Underline (with a style).
    #[serde(default)]
    pub underline: bool,
    /// Inverse video.
    #[serde(default)]
    pub inverse: bool,
    /// Dim.
    #[serde(default)]
    pub dim: bool,
    /// Underline style when underlined.
    #[serde(default)]
    pub underline_style: UnderlineStyle,
}

impl Default for TerminalStyle {
    fn default() -> Self {
        Self {
            fg: TerminalColor::Default,
            bg: TerminalColor::Default,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
            dim: false,
            underline_style: UnderlineStyle::None,
        }
    }
}

/// OSC 8 hyperlink (URL is not dereferenced by the protocol model).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hyperlink {
    /// Optional hyperlink id.
    #[serde(default)]
    pub id: Option<String>,
    /// The URL.
    pub url: String,
}

/// Image placeholder referencing out-of-band payload.
///
/// The record carries only a reference (`image_id`) plus the cell footprint,
/// never the image bytes, so terminal records stay bounded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImagePlaceholder {
    /// Reference to an out-of-band image payload.
    pub image_id: String,
    /// Width in columns.
    pub columns: u16,
    /// Height in rows.
    pub rows: u16,
}

/// A single terminal cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCell {
    /// Text (None for empty/wide-continuation cells).
    #[serde(default)]
    pub text: Option<String>,
    /// Style.
    #[serde(default)]
    pub style: TerminalStyle,
    /// Optional hyperlink.
    #[serde(default)]
    pub hyperlink: Option<Hyperlink>,
    /// Optional image placeholder.
    #[serde(default)]
    pub image: Option<ImagePlaceholder>,
    /// True when this cell is the right half of a wide (2-column) grapheme
    /// cluster; the base cell at `col - 1` holds the text.
    #[serde(default)]
    pub wide_continuation: bool,
}

impl TerminalCell {
    /// An empty cell.
    pub fn empty() -> Self {
        Self {
            text: None,
            style: TerminalStyle::default(),
            hyperlink: None,
            image: None,
            wide_continuation: false,
        }
    }

    /// A cell with a single character and default style.
    pub fn char(character: char) -> Self {
        Self {
            text: Some(character.to_string()),
            style: TerminalStyle::default(),
            hyperlink: None,
            image: None,
            wide_continuation: false,
        }
    }

    /// A cell holding a full grapheme cluster (e.g. a combining sequence or a
    /// ZWJ emoji) with default style.
    pub fn cluster(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            style: TerminalStyle::default(),
            hyperlink: None,
            image: None,
            wide_continuation: false,
        }
    }

    /// The right-half continuation cell of a wide (2-column) grapheme cluster.
    /// It carries the same style as its base cell so backgrounds render
    /// continuously; `text` stays `None`.
    pub fn wide_continuation(style: TerminalStyle) -> Self {
        Self {
            text: None,
            style,
            hyperlink: None,
            image: None,
            wide_continuation: true,
        }
    }
}

/// A row of cells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRow {
    /// Cells, indexed by column.
    #[serde(default)]
    pub cells: Vec<TerminalCell>,
}

impl TerminalRow {
    /// The visible width (cell count).
    pub fn width(&self) -> usize {
        self.cells.len()
    }
}

/// Cursor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorState {
    /// 0-based row.
    pub row: u16,
    /// 0-based column.
    pub col: u16,
    /// Whether the cursor is visible.
    pub visible: bool,
}

/// An unknown/known tagged extension.
///
/// Unknown extensions are safely ignored by consumers: they never fail
/// parsing and are skipped by [`TerminalSnapshot::known_extensions`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Extension {
    /// Extension name, e.g. `osc8.footer.v1`.
    pub name: String,
    /// Opaque payload.
    #[serde(default)]
    pub payload: Vec<u8>,
}

/// A full terminal snapshot (bulk transport / recovery after a gap).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshot {
    /// Stream identifier.
    pub stream_id: u64,
    /// Monotonic per-stream sequence.
    pub sequence: u64,
    /// Visible rows.
    #[serde(default)]
    pub rows: Vec<TerminalRow>,
    /// Cursor.
    pub cursor: CursorState,
    /// Optional window title.
    #[serde(default)]
    pub title: Option<String>,
    /// Optional working directory (OSC 7).
    #[serde(default)]
    pub working_directory: Option<String>,
    /// Absolute line number of the first row in `rows` (scrollback origin).
    pub scrollback_start: u64,
    /// Extensions (unknown names are ignorable).
    #[serde(default)]
    pub extensions: Vec<Extension>,
}

impl TerminalSnapshot {
    /// Extensions with known names (filtered; unknown are ignored).
    pub fn known_extensions(&self, known_names: &[&str]) -> Vec<&Extension> {
        self.extensions
            .iter()
            .filter(|extension| known_names.iter().any(|name| *name == extension.name))
            .collect()
    }

    /// Extensions the receiver does not know; these are safely ignored.
    pub fn unknown_extensions(&self, known_names: &[&str]) -> Vec<&Extension> {
        self.extensions
            .iter()
            .filter(|extension| !known_names.iter().any(|name| *name == extension.name))
            .collect()
    }
}

/// A batch of terminal messages (delta or full snapshot) for one stream.
///
/// Records are transported in versioned length-delimited batches; the
/// protocol model never emits per-character records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalBatch {
    /// Stream identifier.
    pub stream_id: u64,
    /// First sequence covered by the batch.
    pub sequence: u64,
    /// Messages in the batch.
    pub messages: Vec<TerminalMessage>,
}

/// A terminal message: an incremental delta or a full snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalMessage {
    /// Incremental change.
    Delta(TerminalDelta),
    /// Full snapshot.
    Full(TerminalSnapshot),
}

/// Incremental terminal change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDelta {
    /// Stream identifier.
    pub stream_id: u64,
    /// Sequence the delta applies from.
    pub from_sequence: u64,
    /// Sequence the delta applies to.
    pub to_sequence: u64,
    /// Operations.
    #[serde(default)]
    pub operations: Vec<DeltaOp>,
    /// Extensions (unknown names are ignorable).
    #[serde(default)]
    pub extensions: Vec<Extension>,
}

/// A delta operation. Kinds match `render_op_kinds` in the terminal contract
/// plus an image operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeltaOp {
    /// Fill a run with text and style.
    Fill {
        /// 0-based row.
        row: u16,
        /// 0-based column.
        col: u16,
        /// Text (may be empty for a styled clear run).
        text: String,
        /// Style.
        style: TerminalStyle,
    },
    /// Copy a row.
    Copy {
        /// Source row.
        from_row: u16,
        /// Target row.
        to_row: u16,
    },
    /// Clear a rectangle.
    Clear {
        /// Top row.
        row: u16,
        /// Left column.
        col: u16,
        /// Row count.
        rows: u16,
        /// Column count.
        cols: u16,
    },
    /// Move the cursor.
    Cursor {
        /// New cursor.
        cursor: CursorState,
    },
    /// Set the title (None clears it).
    Title {
        /// Optional title.
        title: Option<String>,
    },
    /// Place an image.
    Image {
        /// Row.
        row: u16,
        /// Column.
        col: u16,
        /// Image placeholder.
        image: ImagePlaceholder,
    },
}

/// Terminal protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalProtocolVersion {
    /// Major version: changes break compatibility and require migration.
    pub major: u16,
    /// Minor version: additive, optional-only changes.
    pub minor: u16,
}

impl TerminalProtocolVersion {
    /// Current version implemented by this crate.
    pub const fn current() -> Self {
        Self { major: 1, minor: 0 }
    }

    /// Whether `other` is compatible with this version.
    ///
    /// Compatible means: same major, and `other.minor <= self.minor` for
    /// backward compatibility (we can read older records) or
    /// `self.minor <= other.minor` for forward compatibility with additive
    /// optional fields.
    pub fn compatible_with(self, other: Self) -> bool {
        self.major == other.major
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CursorState, DeltaOp, Extension, Hyperlink, ImagePlaceholder, TerminalBatch, TerminalCell,
        TerminalColor, TerminalDelta, TerminalMessage, TerminalProtocolVersion, TerminalRow,
        TerminalSnapshot, TerminalStyle, UnderlineStyle,
    };

    const KNOWN_EXTENSIONS: [&str; 1] = ["image.kitty.v1"];

    fn sample_snapshot() -> TerminalSnapshot {
        let style = TerminalStyle {
            bold: true,
            fg: TerminalColor::TrueColor {
                r: 255,
                g: 128,
                b: 0,
            },
            ..TerminalStyle::default()
        };
        let row = TerminalRow {
            cells: vec![
                TerminalCell::char('h'),
                TerminalCell::char('i'),
                TerminalCell {
                    text: None,
                    style: style.clone(),
                    hyperlink: Some(Hyperlink {
                        id: Some("link-1".to_owned()),
                        url: "https://example.com".to_owned(),
                    }),
                    image: Some(ImagePlaceholder {
                        image_id: "img-1".to_owned(),
                        columns: 4,
                        rows: 2,
                    }),
                    wide_continuation: false,
                },
            ],
        };
        TerminalSnapshot {
            stream_id: 7,
            sequence: 42,
            rows: vec![row],
            cursor: CursorState {
                row: 0,
                col: 2,
                visible: true,
            },
            title: Some("ssh: prod".to_owned()),
            working_directory: Some("/home/user".to_owned()),
            scrollback_start: 0,
            extensions: vec![
                Extension {
                    name: "image.kitty.v1".to_owned(),
                    payload: vec![1, 2, 3],
                },
                Extension {
                    name: "future.unknown.v9".to_owned(),
                    payload: vec![9],
                },
            ],
        }
    }

    #[test]
    fn style_and_cell_builders_work() {
        assert_eq!(
            TerminalStyle::default().underline_style,
            UnderlineStyle::None
        );
        let cell = TerminalCell::char('x');
        assert_eq!(cell.text.as_deref(), Some("x"));
        assert!(cell.hyperlink.is_none());
        assert!(cell.image.is_none());
        assert!(TerminalCell::empty().text.is_none());
        let row = TerminalRow {
            cells: vec![TerminalCell::empty(); 80],
        };
        assert_eq!(row.width(), 80);
    }

    #[test]
    fn wide_continuation_cell_round_trips() {
        let base = TerminalCell::cluster("?");
        assert_eq!(base.text.as_deref(), Some("?"));
        assert!(!base.wide_continuation);
        let cont = TerminalCell::wide_continuation(TerminalStyle::default());
        assert!(cont.text.is_none());
        assert!(cont.wide_continuation);
        // Serialization includes the marker and old records without it parse
        // with the default (false).
        let json = serde_json::to_string(&cont).expect("serialize");
        assert!(json.contains("\"wide_continuation\":true"));
        let old: TerminalCell = serde_json::from_str(
            r#"{"text":null,"style":{"fg":"Default","bg":"Default","bold":false,"italic":false,"underline":false,"inverse":false,"dim":false,"underline_style":"None"},"hyperlink":null,"image":null}"#,
        )
        .expect("old record parses");
        assert!(!old.wide_continuation);
    }

    #[test]
    fn unknown_extensions_are_safely_ignorable() {
        let snapshot = sample_snapshot();
        let known = snapshot.known_extensions(&KNOWN_EXTENSIONS);
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].name, "image.kitty.v1");
        let unknown = snapshot.unknown_extensions(&KNOWN_EXTENSIONS);
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].name, "future.unknown.v9");
    }

    #[test]
    fn snapshot_golden_serialization_is_stable() {
        // Golden: deterministic JSON for the sample snapshot.
        let snapshot = sample_snapshot();
        let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
        let expected = r#"{"stream_id":7,"sequence":42,"rows":[{"cells":[{"text":"h","style":{"fg":"Default","bg":"Default","bold":false,"italic":false,"underline":false,"inverse":false,"dim":false,"underline_style":"None"},"hyperlink":null,"image":null,"wide_continuation":false},{"text":"i","style":{"fg":"Default","bg":"Default","bold":false,"italic":false,"underline":false,"inverse":false,"dim":false,"underline_style":"None"},"hyperlink":null,"image":null,"wide_continuation":false},{"text":null,"style":{"fg":{"TrueColor":{"r":255,"g":128,"b":0}},"bg":"Default","bold":true,"italic":false,"underline":false,"inverse":false,"dim":false,"underline_style":"None"},"hyperlink":{"id":"link-1","url":"https://example.com"},"image":{"image_id":"img-1","columns":4,"rows":2},"wide_continuation":false}]}],"cursor":{"row":0,"col":2,"visible":true},"title":"ssh: prod","working_directory":"/home/user","scrollback_start":0,"extensions":[{"name":"image.kitty.v1","payload":[1,2,3]},{"name":"future.unknown.v9","payload":[9]}]}"#;
        assert_eq!(json, expected, "golden JSON must be byte-for-byte stable");
        let decoded: TerminalSnapshot = serde_json::from_str(expected).expect("deserialize golden");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn backward_compatible_old_record_parses_with_defaults() {
        // Older records omit optional fields (working_directory,
        // extensions); serde defaults keep them parseable.
        let old = r#"{"stream_id":1,"sequence":5,"rows":[],"cursor":{"row":0,"col":0,"visible":true},"scrollback_start":0}"#;
        let decoded: TerminalSnapshot = serde_json::from_str(old).expect("old record parses");
        assert_eq!(decoded.title, None);
        assert_eq!(decoded.working_directory, None);
        assert!(decoded.extensions.is_empty());
        assert_eq!(decoded.stream_id, 1);
    }

    #[test]
    fn forward_compatible_unknown_extension_is_ignored() {
        // A record with an unknown extension name still parses and the
        // unknown extension is skipped by known_extensions.
        let future = r#"{"stream_id":1,"sequence":5,"rows":[],"cursor":{"row":0,"col":0,"visible":true},"scrollback_start":0,"extensions":[{"name":"future.extension.v99","payload":[]}]}"#;
        let decoded: TerminalSnapshot = serde_json::from_str(future).expect("future record parses");
        assert_eq!(decoded.unknown_extensions(&[]).len(), 1);
        assert!(decoded.known_extensions(&KNOWN_EXTENSIONS).is_empty());
    }

    #[test]
    fn delta_operations_cover_contract_kinds() {
        let delta = TerminalDelta {
            stream_id: 1,
            from_sequence: 10,
            to_sequence: 11,
            operations: vec![
                DeltaOp::Fill {
                    row: 0,
                    col: 0,
                    text: "ok".to_owned(),
                    style: TerminalStyle::default(),
                },
                DeltaOp::Copy {
                    from_row: 0,
                    to_row: 1,
                },
                DeltaOp::Clear {
                    row: 0,
                    col: 0,
                    rows: 1,
                    cols: 2,
                },
                DeltaOp::Cursor {
                    cursor: CursorState {
                        row: 0,
                        col: 2,
                        visible: true,
                    },
                },
                DeltaOp::Title {
                    title: Some("t".to_owned()),
                },
                DeltaOp::Image {
                    row: 0,
                    col: 0,
                    image: ImagePlaceholder {
                        image_id: "i".to_owned(),
                        columns: 2,
                        rows: 1,
                    },
                },
            ],
            extensions: vec![],
        };
        let batch = TerminalBatch {
            stream_id: 1,
            sequence: 10,
            messages: vec![TerminalMessage::Delta(delta)],
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        let decoded: TerminalBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(decoded, batch);
    }

    #[test]
    fn version_compatibility_requires_same_major() {
        let current = TerminalProtocolVersion::current();
        assert!(current.compatible_with(TerminalProtocolVersion { major: 1, minor: 0 }));
        assert!(current.compatible_with(TerminalProtocolVersion { major: 1, minor: 5 }));
        assert!(!current.compatible_with(TerminalProtocolVersion { major: 2, minor: 0 }));
    }
}
