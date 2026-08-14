//! Color palette and indexed-color resolution (T065).
//!
//! [`Palette`] holds the configurable default fg/bg plus the 16 ANSI colors
//! (8 regular + 8 bright) and resolves any [`TerminalColor`] to an [`Rgb`]
//! for the renderer. Indices 16..=231 use the standard 6x6x6 color cube and
//! 232..=255 the 24-step grayscale ramp (xterm convention).

use core_protocol::terminal::TerminalColor;

/// An 8-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

/// The configurable terminal palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Default foreground (used when a cell style has no explicit color).
    pub default_fg: Rgb,
    /// Default background.
    pub default_bg: Rgb,
    /// The 8 regular ANSI colors (indices 0..=7).
    pub regular: [Rgb; 8],
    /// The 8 bright ANSI colors (indices 8..=15).
    pub bright: [Rgb; 8],
}

impl Default for Palette {
    /// The classic xterm palette.
    fn default() -> Self {
        Self {
            default_fg: Rgb {
                r: 0xD0,
                g: 0xD0,
                b: 0xD0,
            },
            default_bg: Rgb {
                r: 0x00,
                g: 0x00,
                b: 0x00,
            },
            regular: [
                Rgb {
                    r: 0x00,
                    g: 0x00,
                    b: 0x00,
                }, // black
                Rgb {
                    r: 0xCD,
                    g: 0x00,
                    b: 0x00,
                }, // red
                Rgb {
                    r: 0x00,
                    g: 0xCD,
                    b: 0x00,
                }, // green
                Rgb {
                    r: 0xCD,
                    g: 0xCD,
                    b: 0x00,
                }, // yellow
                Rgb {
                    r: 0x00,
                    g: 0x00,
                    b: 0xEE,
                }, // blue
                Rgb {
                    r: 0xCD,
                    g: 0x00,
                    b: 0xCD,
                }, // magenta
                Rgb {
                    r: 0x00,
                    g: 0xCD,
                    b: 0xCD,
                }, // cyan
                Rgb {
                    r: 0xE5,
                    g: 0xE5,
                    b: 0xE5,
                }, // white
            ],
            bright: [
                Rgb {
                    r: 0x7F,
                    g: 0x7F,
                    b: 0x7F,
                }, // bright black
                Rgb {
                    r: 0xFF,
                    g: 0x00,
                    b: 0x00,
                }, // bright red
                Rgb {
                    r: 0x00,
                    g: 0xFF,
                    b: 0x00,
                }, // bright green
                Rgb {
                    r: 0xFF,
                    g: 0xFF,
                    b: 0x00,
                }, // bright yellow
                Rgb {
                    r: 0x5C,
                    g: 0x5C,
                    b: 0xFF,
                }, // bright blue
                Rgb {
                    r: 0xFF,
                    g: 0x00,
                    b: 0xFF,
                }, // bright magenta
                Rgb {
                    r: 0x00,
                    g: 0xFF,
                    b: 0xFF,
                }, // bright cyan
                Rgb {
                    r: 0xFF,
                    g: 0xFF,
                    b: 0xFF,
                }, // bright white
            ],
        }
    }
}

impl Palette {
    /// Resolves any [`TerminalColor`] to an [`Rgb`] under this palette.
    pub fn resolve(&self, color: TerminalColor) -> Rgb {
        match color {
            TerminalColor::Default => self.default_fg,
            TerminalColor::Indexed(index) => self.resolve_indexed(index),
            TerminalColor::TrueColor { r, g, b } => Rgb { r, g, b },
        }
    }

    /// Resolves an indexed color (0..=255) to [`Rgb`].
    ///
    /// 0..=15 use the ANSI palette; 16..=231 use the 6x6x6 cube; 232..=255 use
    /// the 24-step grayscale ramp.
    pub fn resolve_indexed(&self, index: u8) -> Rgb {
        match index {
            0..=7 => self.regular[index as usize],
            8..=15 => self.bright[(index - 8) as usize],
            16..=231 => {
                let value = index - 16;
                let r = value / 36;
                let g = (value % 36) / 6;
                let b = value % 6;
                Rgb {
                    r: CUBE_LEVELS[r as usize],
                    g: CUBE_LEVELS[g as usize],
                    b: CUBE_LEVELS[b as usize],
                }
            }
            232..=255 => {
                let level = 8 + 10 * (index - 232);
                Rgb {
                    r: level,
                    g: level,
                    b: level,
                }
            }
        }
    }
}

/// The 6 levels of the 6x6x6 color cube (xterm convention).
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

#[cfg(test)]
mod tests {
    use super::{Palette, Rgb};
    use core_protocol::terminal::TerminalColor;

    #[test]
    fn default_palette_has_expected_anchors() {
        let palette = Palette::default();
        assert_eq!(palette.resolve_indexed(0), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            palette.resolve_indexed(9),
            Rgb {
                r: 0xFF,
                g: 0,
                b: 0
            }
        );
        assert_eq!(
            palette.resolve_indexed(15),
            Rgb {
                r: 0xFF,
                g: 0xFF,
                b: 0xFF
            }
        );
    }

    #[test]
    fn cube_and_grayscale_resolution() {
        let palette = Palette::default();
        // 196 = 16 + 36*5 + 6*0 + 0 -> pure red.
        assert_eq!(palette.resolve_indexed(196), Rgb { r: 255, g: 0, b: 0 });
        // 21 = 16 + 36*0 + 6*0 + 5 -> pure blue.
        assert_eq!(palette.resolve_indexed(21), Rgb { r: 0, g: 0, b: 255 });
        // 232 -> 8, 255 -> 238.
        assert_eq!(palette.resolve_indexed(232), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(
            palette.resolve_indexed(255),
            Rgb {
                r: 238,
                g: 238,
                b: 238
            }
        );
    }

    #[test]
    fn resolve_covers_all_color_kinds() {
        let palette = Palette::default();
        assert_eq!(palette.resolve(TerminalColor::Default), palette.default_fg);
        assert_eq!(
            palette.resolve(TerminalColor::Indexed(1)),
            Rgb {
                r: 0xCD,
                g: 0,
                b: 0
            }
        );
        assert_eq!(
            palette.resolve(TerminalColor::TrueColor {
                r: 10,
                g: 20,
                b: 30
            }),
            Rgb {
                r: 10,
                g: 20,
                b: 30
            }
        );
    }
}
