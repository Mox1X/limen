use limen::{
    ascii::{
        AsciiOptions, CharGrid, GlyphMode, ToneMap, brightness_char, resolve, resolve_with_options,
        resolve_with_tone,
    },
    glyphs::{SHAPE_GLYPHS, match_glyph},
    render::Framebuffer,
};

fn pattern_frame(mask: [f32; 8], repeat: usize) -> Framebuffer {
    let mut frame = Framebuffer::new(2 * repeat, 4 * repeat);
    for y in 0..4 * repeat {
        for x in 0..2 * repeat {
            frame.write(x, y, 0.5, mask[(y / repeat) * 2 + x / repeat]);
        }
    }
    frame
}
fn resolved(frame: &Framebuffer, mode: GlyphMode, tone: ToneMap) -> u8 {
    let mut grid = CharGrid::new(1, 1);
    resolve_with_options(
        frame,
        frame.width(),
        frame.height(),
        &mut grid,
        AsciiOptions {
            glyph_mode: mode,
            tone,
            palette: if mode == GlyphMode::Brightness {
                limen::glyphs::GlyphPalette::Tonal
            } else {
                limen::glyphs::GlyphPalette::Structural
            },
            complexity_penalty: 0.0,
            stability: 0.0,
        },
    );
    grid.get(0, 0).unwrap()
}

#[test]
fn equal_mean_diagonals_are_distinguished_by_shape() {
    let slash = pattern_frame([0., 1., 0.15, 0.85, 0.85, 0.15, 1., 0.], 1);
    let backslash = pattern_frame([1., 0., 0.85, 0.15, 0.15, 0.85, 0., 1.], 1);
    assert_eq!(
        resolved(&slash, GlyphMode::Brightness, ToneMap::LINEAR),
        resolved(&backslash, GlyphMode::Brightness, ToneMap::LINEAR)
    );
    assert_eq!(resolved(&slash, GlyphMode::Shape, ToneMap::LINEAR), b'/');
    assert_eq!(
        resolved(&backslash, GlyphMode::Shape, ToneMap::LINEAR),
        b'\\'
    );
}

#[test]
fn horizontal_stroke_position_survives_resampling() {
    for (pattern, expected) in [
        ([0., 0., 1., 1., 0., 0., 0., 0.], b'-'),
        ([0., 0., 0., 0., 0., 0., 1., 1.], b'_'),
    ] {
        for repeat in [1, 2, 4] {
            assert_eq!(
                resolved(
                    &pattern_frame(pattern, repeat),
                    GlyphMode::Shape,
                    ToneMap::LINEAR
                ),
                expected
            );
        }
    }
}

#[test]
fn shape_applies_tone_before_matching_and_keeps_empty_subregions_black() {
    let tone = ToneMap {
        contrast: 0.5,
        gamma: 2.0,
    };
    let values = [0.0, 1.0, 0.0, 0.6, 0.0, 0.6, 1.0, 0.0];
    let mut frame = Framebuffer::new(2, 4);
    for (i, &value) in values.iter().enumerate() {
        if value > 0.0 {
            frame.write(i % 2, i / 2, 0.5, value);
        }
    }
    let mapped = values.map(|v| if v == 0.0 { 0.0 } else { tone.apply(v) });
    assert_eq!(
        resolved(&frame, GlyphMode::Shape, tone),
        match_glyph(mapped)
    );
    assert_eq!(
        resolved(&Framebuffer::new(2, 4), GlyphMode::Shape, tone),
        b' '
    );
    // A fully covered but unlit surface is distinct from empty background.
    let dark = pattern_frame([0.0; 8], 1);
    assert_eq!(
        resolved(&dark, GlyphMode::Shape, tone),
        brightness_char(tone.apply(0.0))
    );
}

#[test]
fn existing_resolvers_keep_their_brightness_behavior() {
    let frame = pattern_frame(SHAPE_GLYPHS[0].mask, 1);
    let mut a = CharGrid::new(1, 1);
    let mut b = CharGrid::new(1, 1);
    resolve(&frame, 2, 4, &mut a);
    resolve_with_tone(&frame, 2, 4, &mut b, ToneMap::LINEAR);
    assert_eq!(a.get(0, 0), b.get(0, 0));
    assert_eq!(
        a.get(0, 0),
        Some(resolved(&frame, GlyphMode::Brightness, ToneMap::LINEAR))
    );
    for value in [0.0, 0.15, 0.5, 0.8, 1.0] {
        let frame = pattern_frame([value; 8], 1);
        assert_eq!(
            resolved(&frame, GlyphMode::Shape, ToneMap::default()),
            resolved(&frame, GlyphMode::Brightness, ToneMap::default())
        );
    }
}
