use limen::{
    ascii::{AsciiOptions, AsciiResolver, CharGrid, GlyphMode, GlyphPalette, RAMP, ToneMap},
    font_atlas::{FontAtlasConfig, GlyphAtlas},
    geometry::{self, Camera, Scene},
    render::{Framebuffer, RenderOptions, render_with_options},
    view::ViewMode,
};
const FONT: &[u8] = include_bytes!("fixtures/fonts/JetBrainsMono-Regular.ttf");
fn pattern(values: [f32; 8]) -> Framebuffer {
    let mut frame = Framebuffer::new(2, 4);
    for (i, v) in values.into_iter().enumerate() {
        frame.write(i % 2, i / 2, 0.5, v);
    }
    frame
}
fn character(r: &mut AsciiResolver, frame: &Framebuffer, options: AsciiOptions) -> u8 {
    let mut grid = CharGrid::new(1, 1);
    r.resolve(frame, 2, 4, &mut grid, options);
    grid.get(0, 0).unwrap()
}
#[test]
fn every_algorithm_obeys_its_palette_with_and_without_a_font() {
    for has_font in [false, true] {
        let mut r = if has_font {
            AsciiResolver::with_atlas(
                GlyphAtlas::from_bytes(FONT, FontAtlasConfig::default()).unwrap(),
            )
        } else {
            AsciiResolver::default()
        };
        for palette in GlyphPalette::ALL {
            if !has_font && palette == GlyphPalette::Full {
                continue;
            }
            for glyph_mode in [GlyphMode::Brightness, GlyphMode::Shape, GlyphMode::Hybrid] {
                for i in 0..=100 {
                    let values = std::array::from_fn(|j| ((i * 17 + j * 31) % 101) as f32 / 100.0);
                    let symbol = character(
                        &mut r,
                        &pattern(values),
                        AsciiOptions {
                            palette,
                            glyph_mode,
                            ..Default::default()
                        },
                    );
                    assert!(
                        palette.contains(symbol),
                        "{palette}/{glyph_mode}: {}",
                        symbol as char
                    );
                    assert!(!symbol.is_ascii_alphanumeric() || palette == GlyphPalette::Full);
                }
            }
        }
    }
}
#[test]
fn temporal_hold_is_small_and_clears_on_empty_cells_options_and_explicit_reset() {
    let options = AsciiOptions {
        glyph_mode: GlyphMode::Shape,
        palette: GlyphPalette::Tonal,
        tone: ToneMap::LINEAR,
        ..Default::default()
    };
    let mut r = AsciiResolver::default();
    assert_eq!(character(&mut r, &pattern([0.499; 8]), options), b'=');
    assert_eq!(character(&mut r, &pattern([0.501; 8]), options), b'=');
    let mut fresh = AsciiResolver::default();
    assert_eq!(character(&mut fresh, &pattern([0.501; 8]), options), b'+');
    r.reset_history();
    assert_eq!(character(&mut r, &pattern([0.501; 8]), options), b'+');
    character(
        &mut r,
        &pattern([0.499; 8]),
        AsciiOptions {
            stability: 0.0,
            ..options
        },
    );
    assert_eq!(character(&mut r, &pattern([0.501; 8]), options), b'+');
    character(&mut r, &pattern([0.499; 8]), options);
    assert_eq!(character(&mut r, &Framebuffer::new(2, 4), options), b' ');
    assert_eq!(character(&mut r, &pattern([0.501; 8]), options), b'+');
    for value in [0.499, 0.501, 0.52, 0.54] {
        character(&mut r, &pattern([value; 8]), options);
    }
    assert_eq!(character(&mut r, &pattern([0.56; 8]), options), b'+');
}
#[test]
fn legacy_full_font_shape_selection_remains_reproducible() {
    let mut r = AsciiResolver::with_atlas(
        GlyphAtlas::from_bytes(FONT, FontAtlasConfig::default()).unwrap(),
    );
    let options = AsciiOptions {
        glyph_mode: GlyphMode::Shape,
        palette: GlyphPalette::Full,
        tone: ToneMap::LINEAR,
        complexity_penalty: 0.0,
        stability: 0.0,
    };
    for i in 0..100 {
        let mask = std::array::from_fn(|j| ((i * 17 + j * 31) % 101) as f32 / 100.0);
        let expected = r.atlas().unwrap().select_shape(mask);
        assert_eq!(character(&mut r, &pattern(mask), options), expected);
    }
}
#[test]
fn diagnostics_force_tonal_output_and_bypass_history_and_tone() {
    let scene = Scene::from_mesh(geometry::cube());
    let camera = Camera::fitted(scene.instances[0].mesh.bounding_sphere().unwrap(), 1.5, 1.0);
    let mut frame = Framebuffer::new(48, 64);
    let mut grid = CharGrid::new(24, 16);
    let mut r = AsciiResolver::with_atlas(
        GlyphAtlas::from_bytes(FONT, FontAtlasConfig::default()).unwrap(),
    );
    for mode in ViewMode::ALL.into_iter().skip(1) {
        render_with_options(
            &scene,
            &camera,
            1.5,
            &mut frame,
            RenderOptions {
                mode,
                capture_features: true,
            },
        );
        assert!(frame.normals().is_none());
        r.resolve(
            &frame,
            2,
            4,
            &mut grid,
            AsciiOptions {
                palette: GlyphPalette::Full,
                stability: 1.0,
                complexity_penalty: 1.0,
                tone: ToneMap {
                    contrast: 0.25,
                    gamma: 4.0,
                },
                ..Default::default()
            },
        );
        let a: Vec<_> = grid.rows().flatten().copied().collect();
        assert!(a.iter().all(|c| RAMP.contains(c)));
        r.resolve(&frame, 2, 4, &mut grid, AsciiOptions::default());
        assert_eq!(a, grid.rows().flatten().copied().collect::<Vec<_>>());
    }
}
#[test]
fn different_sampling_and_grid_dimensions_rebuild_history_safely() {
    let mut r = AsciiResolver::default();
    for (w, h, sx, sy) in [
        (2, 2, 2, 4),
        (3, 1, 3, 5),
        (1, 3, 1, 1),
        (0, 0, 2, 4),
        (2, 2, 2, 4),
    ] {
        let mut frame = Framebuffer::new(w * sx, h * sy);
        for y in 0..h * sy {
            for x in 0..w * sx {
                frame.write(x, y, 0.5, 0.5);
            }
        }
        let mut grid = CharGrid::new(w, h);
        r.resolve(&frame, sx, sy, &mut grid, AsciiOptions::default());
        assert!(grid.rows().flatten().all(|&c| RAMP.contains(&c)));
    }
}

#[test]
fn font_tonal_stability_tracks_density_instead_of_stroke_shape() {
    let mut r = AsciiResolver::with_atlas(
        GlyphAtlas::from_bytes(FONT, FontAtlasConfig::default()).unwrap(),
    );
    let options = AsciiOptions {
        palette: GlyphPalette::Tonal,
        tone: ToneMap::LINEAR,
        ..Default::default()
    };
    for step in 0..=500 {
        let value = step as f32 / 500.0;
        let symbol = character(&mut r, &pattern([value; 8]), options);
        let atlas = r.atlas().unwrap();
        let max = atlas
            .glyphs()
            .iter()
            .filter(|g| GlyphPalette::Tonal.contains(g.symbol))
            .map(|g| g.density)
            .fold(0.0_f32, f32::max);
        let actual = (atlas.glyphs()[(symbol - b' ') as usize].density / max - value).powi(2);
        let best = atlas
            .glyphs()
            .iter()
            .filter(|g| GlyphPalette::Tonal.contains(g.symbol))
            .map(|g| (g.density / max - value).powi(2))
            .fold(f32::INFINITY, f32::min);
        assert!(
            actual <= best + options.stability + 1e-6,
            "stale tonal step at {value}"
        );
    }
}

#[test]
fn camera_projection_changes_invalidate_held_tonal_symbols() {
    let mut scene = Scene::from_mesh(geometry::cube());
    scene.light.ambient = 0.499;
    scene.light.diffuse = 0.0;
    let bounds = scene.instances[0].mesh.bounding_sphere().unwrap();
    let mut frame = Framebuffer::new(6, 12);
    let mut grid = CharGrid::new(3, 3);
    let mut r = AsciiResolver::default();
    let options = AsciiOptions {
        glyph_mode: GlyphMode::Shape,
        palette: GlyphPalette::Tonal,
        tone: ToneMap::LINEAR,
        ..Default::default()
    };
    let camera = Camera::fitted(bounds, 0.5, 1.0);
    render_with_options(&scene, &camera, 0.5, &mut frame, RenderOptions::default());
    r.resolve(&frame, 2, 4, &mut grid, options);
    assert_eq!(grid.get(1, 1), Some(b'='));
    scene.light.ambient = 0.501;
    render_with_options(&scene, &camera, 0.5, &mut frame, RenderOptions::default());
    r.resolve(&frame, 2, 4, &mut grid, options);
    assert_eq!(grid.get(1, 1), Some(b'='));
    let zoomed = Camera::fitted(bounds, 0.5, 1.1);
    render_with_options(&scene, &zoomed, 0.5, &mut frame, RenderOptions::default());
    r.resolve(&frame, 2, 4, &mut grid, options);
    assert_eq!(grid.get(1, 1), Some(b'+'));
}
