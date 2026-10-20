use limen::{
    ascii::{AsciiOptions, AsciiResolver, CharGrid, GlyphMode, ToneMap},
    font_atlas::{CellSize, FontAtlasConfig, GlyphAtlas},
    render::Framebuffer,
};
use std::sync::OnceLock;
const FONT: &[u8] = include_bytes!("fixtures/fonts/JetBrainsMono-Regular.ttf");
fn atlas() -> &'static GlyphAtlas {
    static ATLAS: OnceLock<GlyphAtlas> = OnceLock::new();
    ATLAS.get_or_init(|| GlyphAtlas::from_bytes(FONT, FontAtlasConfig::default()).unwrap())
}
#[test]
fn real_atlas_has_finite_masks_and_coverage_based_densities() {
    let atlas = atlas();
    assert_eq!(atlas.glyphs().len(), 95);
    assert_eq!(atlas.glyphs()[0].symbol, b' ');
    assert_eq!(atlas.glyphs()[0].density, 0.0);
    assert!(atlas.max_density() > 0.0 && atlas.max_density() < 1.0);
    for (i, glyph) in atlas.glyphs().iter().enumerate() {
        assert_eq!(glyph.symbol as usize, i + 32);
        assert_eq!(
            glyph.pixels().len(),
            atlas.cell().width * atlas.cell().height
        );
        assert!(
            glyph
                .mask
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        );
        let actual = glyph
            .pixels()
            .iter()
            .map(|&v| v as f32 / 255.0)
            .sum::<f32>()
            / glyph.pixels().len() as f32;
        assert!((actual - glyph.density).abs() < 1e-6);
    }
    let sorted: Vec<_> = atlas.density_order().collect();
    assert!(sorted.windows(2).all(|p| p[0].density <= p[1].density));
}
#[test]
fn dot_and_underscore_keep_their_size_and_shared_baseline() {
    let atlas = atlas();
    let font = fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default()).unwrap();
    let glyph = |c: u8| &atlas.glyphs()[(c - b' ') as usize];
    for c in *b"._@g/" {
        let (_, bitmap) = font.rasterize(c as char, atlas.size_px());
        assert_eq!(
            bitmap.iter().map(|&v| v as u64).sum::<u64>(),
            glyph(c).pixels().iter().map(|&v| v as u64).sum::<u64>(),
            "glyph {} was unexpectedly resized or clipped",
            c as char
        );
    }
    let centroid_y = |c| {
        let pixels = glyph(c).pixels();
        let mass = pixels.iter().map(|&v| v as f32).sum::<f32>();
        pixels
            .iter()
            .enumerate()
            .map(|(i, &v)| (i / atlas.cell().width) as f32 * v as f32)
            .sum::<f32>()
            / mass
    };
    assert!(centroid_y(b'.') > centroid_y(b'@'));
    assert!(centroid_y(b'_') > centroid_y(b'.'));
    assert!(glyph(b'.').density < glyph(b'@').density);
}
#[test]
fn brightness_selection_is_monotone_in_actual_density() {
    let atlas = atlas();
    assert_eq!(atlas.select_brightness(0.0), b' ');
    let brightest = atlas.select_brightness(1.0);
    assert_eq!(
        atlas.glyphs()[(brightest - b' ') as usize].density,
        atlas.max_density()
    );
    let mut previous = 0.0;
    for i in 0..=1000 {
        let v = i as f32 / 1000.0;
        let symbol = atlas.select_brightness(v);
        let density = atlas.glyphs()[(symbol - b' ') as usize].density;
        assert!(density >= previous);
        previous = density;
        assert_eq!(atlas.select_shape([v; 8]), symbol);
        let target = v * atlas.max_density();
        assert!(
            atlas
                .glyphs()
                .iter()
                .all(|g| (density - target).abs() <= (g.density - target).abs() + 1e-7)
        );
    }
}
#[test]
fn resolver_reuses_atlas_across_sampling_modes_and_leaves_background_blank() {
    let mut resolver = AsciiResolver::with_atlas(
        GlyphAtlas::from_bytes(FONT, FontAtlasConfig::default()).unwrap(),
    );
    let tone = ToneMap {
        contrast: 0.5,
        gamma: 2.0,
    };
    for (sx, sy) in [(2, 4), (3, 5), (4, 8), (1, 1), (2, 4)] {
        let mut frame = Framebuffer::new(2 * sx, sy);
        for y in 0..sy {
            for x in 0..sx {
                frame.write(x, y, 0.5, 0.4);
            }
        }
        let mut grid = CharGrid::new(2, 1);
        let expected = resolver.atlas().unwrap().select_brightness(tone.apply(0.4));
        for glyph_mode in [GlyphMode::Brightness, GlyphMode::Shape] {
            resolver.resolve(
                &frame,
                sx,
                sy,
                &mut grid,
                AsciiOptions {
                    glyph_mode,
                    tone,
                    palette: limen::glyphs::GlyphPalette::Full,
                    complexity_penalty: 0.0,
                    stability: 0.0,
                },
            );
            assert_eq!(grid.get(0, 0), Some(expected));
            assert_eq!(grid.get(1, 0), Some(b' '));
        }
    }
}
#[test]
fn explicit_cell_clips_ink_and_bigger_cell_preserves_it_with_padding() {
    let natural = atlas();
    let large = GlyphAtlas::from_bytes(
        FONT,
        FontAtlasConfig {
            cell: Some(CellSize {
                width: natural.cell().width + 6,
                height: natural.cell().height + 8,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let small = GlyphAtlas::from_bytes(
        FONT,
        FontAtlasConfig {
            cell: Some(CellSize {
                width: 4,
                height: 8,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let total = |a: &GlyphAtlas| {
        a.glyphs()
            .iter()
            .flat_map(|g| g.pixels())
            .map(|&v| v as u64)
            .sum::<u64>()
    };
    assert_eq!(large.origin_x(), natural.origin_x() + 3);
    assert_eq!(large.baseline(), natural.baseline() + 4);
    assert_eq!(total(&large), total(natural));
    assert!(total(&small) < total(natural));
    assert!(large.max_density() < natural.max_density());
}
fn table(bytes: &[u8], wanted: &[u8; 4]) -> (usize, usize) {
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    for i in 0..count {
        let record = 12 + i * 16;
        if &bytes[record..record + 4] == wanted {
            let offset =
                u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize;
            let length =
                u32::from_be_bytes(bytes[record + 12..record + 16].try_into().unwrap()) as usize;
            return (offset, length);
        }
    }
    panic!("fixture table not found");
}
#[test]
fn invalid_fonts_sizes_cells_and_empty_coverage_are_errors() {
    assert!(GlyphAtlas::from_bytes(b"not a font", FontAtlasConfig::default()).is_err());
    for size in [0.0, 3.0, 129.0, f32::NAN, f32::INFINITY] {
        assert!(
            GlyphAtlas::from_bytes(
                FONT,
                FontAtlasConfig {
                    size_px: size,
                    cell: None
                }
            )
            .is_err()
        );
    }
    for cell in [
        CellSize {
            width: 0,
            height: 1,
        },
        CellSize {
            width: 1,
            height: 513,
        },
    ] {
        assert!(
            GlyphAtlas::from_bytes(
                FONT,
                FontAtlasConfig {
                    cell: Some(cell),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
    // Modify only in memory: an empty loca table retains metrics but removes outlines.
    let mut empty = FONT.to_vec();
    let (offset, length) = table(&empty, b"loca");
    empty[offset..offset + length].fill(0);
    let error = GlyphAtlas::from_bytes(&empty, FontAtlasConfig::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("no visible coverage"), "{error}");
}
#[test]
fn fonts_with_proportional_advances_are_rejected() {
    let font = fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default()).unwrap();
    let index = font.lookup_glyph_index('A') as usize;
    let mut proportional = FONT.to_vec();
    let (offset, _) = table(&proportional, b"hmtx");
    let at = offset + index * 4;
    let advance = u16::from_be_bytes(proportional[at..at + 2].try_into().unwrap());
    proportional[at..at + 2].copy_from_slice(&(advance + 200).to_be_bytes());
    let error = GlyphAtlas::from_bytes(&proportional, FontAtlasConfig::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("monospaced"), "{error}");
}
#[test]
fn missing_ascii_is_rejected() {
    // A valid but empty character map leaves metrics/outlines intact.
    // Modify only memory; the original fixture remains unmodified.
    let mut missing = FONT.to_vec();
    let (cmap, _) = table(&missing, b"cmap");
    missing[cmap + 2..cmap + 4].copy_from_slice(&0_u16.to_be_bytes());
    let error = GlyphAtlas::from_bytes(&missing, FontAtlasConfig::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("missing ASCII"), "{error}");
}
