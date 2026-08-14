//! Unicode width policy and grapheme segmentation (T064).
//!
//! The width tables come from the `unicode-width` crate; the Unicode version
//! they implement is locked here as [`UNICODE_VERSION`] and asserted against
//! the crate by a test. Grapheme cluster segmentation (UAX #29, extended
//! clusters) comes from `unicode-segmentation`. The screen model consumes
//! clusters, so combining sequences and ZWJ emoji occupy a single cell with
//! the width of their base.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Locked Unicode version implemented by the width tables
/// (`unicode-width` 0.2.x, verified by `unicode_version_matches_tables`).
pub const UNICODE_VERSION: &str = "17.0.0";

/// Configurable column-width policy.
///
/// Terminals disagree on how to render East Asian Ambiguous (EAW=A)
/// characters and whether wide characters exist at all; the policy is
/// selectable at runtime so the same model can serve legacy and CJK hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WidthPolicy {
    /// UAX #11 non-CJK context: Ambiguous characters are 1 column.
    #[default]
    Unicode,
    /// CJK context: Ambiguous characters are 2 columns.
    EastAsian,
    /// Legacy: every printable character occupies exactly 1 column.
    Legacy,
}

impl WidthPolicy {
    /// Display width of a single character under this policy.
    pub fn char_width(self, ch: char) -> usize {
        match self {
            WidthPolicy::Unicode => UnicodeWidthChar::width(ch).unwrap_or(0),
            WidthPolicy::EastAsian => UnicodeWidthChar::width_cjk(ch).unwrap_or(0),
            WidthPolicy::Legacy => {
                if ch.is_control() {
                    0
                } else {
                    1
                }
            }
        }
    }

    /// Display width of a whole string under this policy.
    pub fn str_width(self, text: &str) -> usize {
        match self {
            WidthPolicy::Unicode => UnicodeWidthStr::width(text),
            WidthPolicy::EastAsian => UnicodeWidthStr::width_cjk(text),
            WidthPolicy::Legacy => text
                .chars()
                .map(|ch| if ch.is_control() { 0 } else { 1 })
                .sum(),
        }
    }

    /// Display width of one grapheme cluster under this policy.
    pub fn cluster_width(self, cluster: &str) -> usize {
        self.str_width(cluster)
    }
}

/// Segments `text` into extended grapheme clusters (UAX #29).
pub fn grapheme_clusters(text: &str) -> Vec<&str> {
    text.graphemes(true).collect()
}

#[cfg(test)]
mod tests {
    use super::{grapheme_clusters, WidthPolicy, UNICODE_VERSION};

    #[test]
    fn unicode_version_is_locked_and_matches_tables() {
        // The version is a locked contract; it must match the width tables.
        let (major, minor, patch) = unicode_width::UNICODE_VERSION;
        assert_eq!(
            UNICODE_VERSION,
            format!("{major}.{minor}.{patch}"),
            "UNICODE_VERSION must match unicode-width tables"
        );
    }

    #[test]
    fn ascii_and_cjk_widths() {
        assert_eq!(WidthPolicy::Unicode.char_width('a'), 1);
        assert_eq!(WidthPolicy::Unicode.char_width('中'), 2);
        assert_eq!(WidthPolicy::EastAsian.char_width('中'), 2);
        assert_eq!(WidthPolicy::Legacy.char_width('中'), 1);
        assert_eq!(WidthPolicy::Legacy.char_width('\n'), 0);
        assert_eq!(WidthPolicy::Unicode.str_width("a中b"), 4);
    }

    #[test]
    fn ambiguous_width_depends_on_policy() {
        // U+00B7 MIDDLE DOT is East Asian Ambiguous.
        let dot = '\u{00b7}';
        assert_eq!(WidthPolicy::Unicode.char_width(dot), 1);
        assert_eq!(WidthPolicy::EastAsian.char_width(dot), 2);
    }

    #[test]
    fn combining_and_emoji_cluster_widths() {
        // e + COMBINING ACUTE ACCENT is one grapheme, width 1.
        let e_acute = "e\u{0301}";
        assert_eq!(grapheme_clusters(e_acute), vec!["e\u{0301}"]);
        assert_eq!(WidthPolicy::Unicode.cluster_width(e_acute), 1);
        // Family emoji ZWJ sequence is one grapheme, width 2.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(grapheme_clusters(family), vec![family]);
        assert_eq!(WidthPolicy::Unicode.cluster_width(family), 2);
        // A lone combining mark is a zero-width cluster.
        assert_eq!(WidthPolicy::Unicode.cluster_width("\u{0301}"), 0);
    }
}
