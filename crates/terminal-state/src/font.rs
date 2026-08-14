//! Font discovery, fallback, bold/italic, and variable-font policy (T074).
//!
//! Terminal text mixes scripts (Latin, CJK, emoji, Powerline symbols). A
//! [`FallbackPolicy`] maps each script to a font family and each
//! [`FontStyle`] to a weight / italic axis, so missing glyphs fall back
//! predictably (Windows-first defaults). The cross-platform screenshot
//! coverage matrix requires a real renderer/GPU and is `blocked_environment`
//! on CI hosts without one; the deterministic per-character resolution is
//! covered by unit tests.

/// Text style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    /// Regular weight, upright.
    Normal,
    /// Heavy weight, upright.
    Bold,
    /// Regular weight, slanted.
    Italic,
    /// Heavy weight, slanted.
    BoldItalic,
}

/// A resolved font choice for one glyph.
#[derive(Debug, Clone, PartialEq)]
pub struct FontSpec {
    /// Font family.
    pub family: String,
    /// Weight axis value (variable-font weight; e.g. 400 normal / 700 bold).
    pub weight: u16,
    /// Italic axis.
    pub italic: bool,
    /// Point size.
    pub size_pt: f32,
}

/// A coarse script classification for fallback selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// Latin / ASCII.
    Latin,
    /// CJK ideographs, kana, hangul.
    Cjk,
    /// Emoji and pictographs.
    Emoji,
    /// Powerline / Nerd-Font symbols.
    Powerline,
    /// Everything else.
    Other,
}

/// Classifies a character into a script for fallback selection.
pub fn script_for_char(ch: char) -> Script {
    match ch as u32 {
        0x0000..=0x024f | 0x1e00..=0x1eff | 0x2c60..=0x2c7f => Script::Latin,
        0x2e80..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff => Script::Cjk,
        0x1f000..=0x1faff | 0x2600..=0x27bf | 0xfe0f | 0x200d => Script::Emoji,
        0xe000..=0xe0ff | 0x2b60..=0x2bff => Script::Powerline,
        _ => Script::Other,
    }
}

/// Configurable font fallback and variable-font policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackPolicy {
    /// Family for Latin / default text.
    pub latin: String,
    /// Family for CJK text.
    pub cjk: String,
    /// Family for emoji.
    pub emoji: String,
    /// Family for Powerline / Nerd-Font symbols.
    pub powerline: String,
    /// Weight for normal text (variable-font axis).
    pub normal_weight: u16,
    /// Weight for bold text.
    pub bold_weight: u16,
}

impl Default for FallbackPolicy {
    /// Windows-first defaults (Tier 1 platform).
    fn default() -> Self {
        Self {
            latin: "Cascadia Mono".to_owned(),
            cjk: "Microsoft YaHei UI".to_owned(),
            emoji: "Segoe UI Emoji".to_owned(),
            powerline: "CaskaydiaCove Nerd Font".to_owned(),
            normal_weight: 400,
            bold_weight: 700,
        }
    }
}

impl FallbackPolicy {
    /// The family for a script.
    pub fn family_for(&self, script: Script) -> &str {
        match script {
            Script::Latin => &self.latin,
            Script::Cjk => &self.cjk,
            Script::Emoji => &self.emoji,
            Script::Powerline => &self.powerline,
            Script::Other => &self.latin,
        }
    }

    /// The weight for a style (variable-font weight axis).
    pub fn weight_for(&self, style: FontStyle) -> u16 {
        match style {
            FontStyle::Normal | FontStyle::Italic => self.normal_weight,
            FontStyle::Bold | FontStyle::BoldItalic => self.bold_weight,
        }
    }

    /// The italic axis for a style.
    pub fn italic_for(&self, style: FontStyle) -> bool {
        matches!(style, FontStyle::Italic | FontStyle::BoldItalic)
    }

    /// Resolves a font for a character under a style: picks the family by
    /// script and the weight/italic axes by style, so missing glyphs fall
    /// back predictably.
    pub fn resolve(&self, ch: char, style: FontStyle, size_pt: f32) -> FontSpec {
        FontSpec {
            family: self.family_for(script_for_char(ch)).to_owned(),
            weight: self.weight_for(style),
            italic: self.italic_for(style),
            size_pt,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{script_for_char, FallbackPolicy, FontStyle, Script};

    #[test]
    fn script_classification_is_predictable() {
        assert_eq!(script_for_char('a'), Script::Latin);
        assert_eq!(script_for_char('中'), Script::Cjk);
        assert_eq!(script_for_char('\u{3042}'), Script::Cjk); // hiragana
        assert_eq!(script_for_char('\u{1F600}'), Script::Emoji);
        assert_eq!(script_for_char('\u{E0B0}'), Script::Powerline);
        assert_eq!(script_for_char('\u{00E9}'), Script::Latin); // é
        assert_eq!(script_for_char(' '), Script::Latin); // space is Latin
    }

    #[test]
    fn fallback_chain_resolves_predictably() {
        let policy = FallbackPolicy::default();
        let text = "ab\u{4e2d}\u{1F600}\u{E0B0}";
        let chain: Vec<String> = text
            .chars()
            .map(|ch| policy.resolve(ch, FontStyle::Normal, 12.0).family)
            .collect();
        assert_eq!(
            chain,
            vec![
                "Cascadia Mono".to_owned(),
                "Cascadia Mono".to_owned(),
                "Microsoft YaHei UI".to_owned(),
                "Segoe UI Emoji".to_owned(),
                "CaskaydiaCove Nerd Font".to_owned()
            ]
        );
    }

    #[test]
    fn styles_map_to_weight_and_italic_axes() {
        let policy = FallbackPolicy::default();
        let normal = policy.resolve('a', FontStyle::Normal, 12.0);
        assert_eq!((normal.weight, normal.italic), (400, false));
        let bold = policy.resolve('a', FontStyle::Bold, 12.0);
        assert_eq!((bold.weight, bold.italic), (700, false));
        let italic = policy.resolve('a', FontStyle::Italic, 12.0);
        assert_eq!((italic.weight, italic.italic), (400, true));
        let bold_italic = policy.resolve('a', FontStyle::BoldItalic, 12.0);
        assert_eq!((bold_italic.weight, bold_italic.italic), (700, true));
    }

    #[test]
    fn size_is_preserved() {
        let policy = FallbackPolicy::default();
        let spec = policy.resolve('中', FontStyle::BoldItalic, 14.5);
        assert_eq!(spec.size_pt, 14.5);
        assert_eq!(spec.family, "Microsoft YaHei UI");
    }
}
