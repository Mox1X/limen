//! Approximate, font-independent 2x4 luminance masks for the experimental matcher.
use crate::ascii::{RAMP, brightness_char};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GlyphMode {
    #[default]
    Brightness,
    Shape,
}
impl fmt::Display for GlyphMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Brightness => "brightness",
            Self::Shape => "shape",
        })
    }
}
impl FromStr for GlyphMode {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "brightness" => Ok(Self::Brightness),
            "shape" => Ok(Self::Shape),
            _ => Err("expected brightness or shape".into()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphTemplate {
    pub symbol: u8,
    /// Row-major: top-left, top-right, second-left, second-right, ...
    pub mask: [f32; 8],
}

// Values describe approximate stroke coverage, not calibrated font rasterization.
// The straight vertical stroke is tapered at its ends; 2 columns cannot resolve
// a thin centered vertical line separately from a uniform tone in every case.
pub const SHAPE_GLYPHS: &[GlyphTemplate] = &[
    GlyphTemplate {
        symbol: b'/',
        mask: [0., 1., 0.15, 0.85, 0.85, 0.15, 1., 0.],
    },
    GlyphTemplate {
        symbol: b'\\',
        mask: [1., 0., 0.85, 0.15, 0.15, 0.85, 0., 1.],
    },
    GlyphTemplate {
        symbol: b'|',
        mask: [0.2, 0.2, 0.4, 0.4, 0.4, 0.4, 0.2, 0.2],
    },
    GlyphTemplate {
        symbol: b'-',
        mask: [0., 0., 1., 1., 0., 0., 0., 0.],
    },
    GlyphTemplate {
        symbol: b'_',
        mask: [0., 0., 0., 0., 0., 0., 1., 1.],
    },
    GlyphTemplate {
        symbol: b'(',
        mask: [0., 0.6, 0.6, 0.05, 0.6, 0.05, 0., 0.6],
    },
    GlyphTemplate {
        symbol: b')',
        mask: [0.6, 0., 0.05, 0.6, 0.05, 0.6, 0.6, 0.],
    },
    GlyphTemplate {
        symbol: b'[',
        mask: [0.8, 0.4, 0.8, 0., 0.8, 0., 0.8, 0.4],
    },
    GlyphTemplate {
        symbol: b']',
        mask: [0.4, 0.8, 0., 0.8, 0., 0.8, 0.4, 0.8],
    },
    GlyphTemplate {
        symbol: b'<',
        mask: [0., 0.8, 0.8, 0., 0.8, 0., 0., 0.8],
    },
    GlyphTemplate {
        symbol: b'>',
        mask: [0.8, 0., 0., 0.8, 0., 0.8, 0.8, 0.],
    },
];

/// Matches an already tone-mapped cell, retaining a uniform-luminance candidate.
pub fn match_glyph(pattern: [f32; 8]) -> u8 {
    best_match(pattern, SHAPE_GLYPHS)
}
fn best_match(pattern: [f32; 8], atlas: &[GlyphTemplate]) -> u8 {
    let mean = pattern.iter().sum::<f32>() / 8.0;
    let mut best = brightness_char(mean);
    let variance = pattern.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / 8.0;
    if variance <= 0.0025 {
        return best;
    }
    // Of all uniform ramp masks, the closest one is determined by the mean.
    // Start with it so exact ties prefer density; other ties retain atlas order.
    let level = RAMP.iter().position(|&c| c == best).unwrap() as f32 / (RAMP.len() - 1) as f32;
    let mut error = pattern.iter().map(|v| (v - level).powi(2)).sum::<f32>();
    for glyph in atlas {
        let candidate = pattern
            .iter()
            .zip(glyph.mask)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>();
        if candidate < error {
            best = glyph.symbol;
            error = candidate;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_atlas_masks_reproduce_their_symbols() {
        for glyph in SHAPE_GLYPHS {
            assert_eq!(match_glyph(glyph.mask), glyph.symbol);
        }
    }
    #[test]
    fn uniform_cells_use_the_existing_ramp() {
        for i in 0..=100 {
            let value = i as f32 / 100.0;
            assert_eq!(match_glyph([value; 8]), brightness_char(value));
        }
        assert_eq!(match_glyph([0.; 8]), b' ');
        assert_eq!(match_glyph([1.; 8]), b'@');
        assert_eq!(
            match_glyph([0.49, 0.51, 0.49, 0.51, 0.49, 0.51, 0.49, 0.51]),
            brightness_char(0.5)
        );
    }
    #[test]
    fn ties_prefer_density_then_atlas_order() {
        let level = 4.0 / 9.0;
        let pattern = [
            level - 0.2,
            level + 0.2,
            level - 0.2,
            level + 0.2,
            level - 0.2,
            level + 0.2,
            level - 0.2,
            level + 0.2,
        ];
        let uniform = GlyphTemplate {
            symbol: b'/',
            mask: [level; 8],
        };
        assert_eq!(best_match(pattern, &[uniform]), brightness_char(level));
        let first = GlyphTemplate {
            symbol: b'/',
            mask: pattern,
        };
        let second = GlyphTemplate {
            symbol: b'\\',
            mask: pattern,
        };
        assert_eq!(best_match(pattern, &[first, second]), b'/');
        assert_eq!(best_match(pattern, &[second, first]), b'\\');
    }
}
