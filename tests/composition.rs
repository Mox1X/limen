use limen::{
    ascii::{AsciiOptions, AsciiResolver, CharGrid, GlyphMode, GlyphPalette, RAMP, ToneMap},
    composition::{Contacts, SplitBackdrop, Viewport, overlay_model},
    render::Framebuffer,
};
fn encode(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
    }
    bytes
}
fn hands() -> Vec<u8> {
    // Odd width, different finger heights/tones, and a transparent white seam.
    let mut pixels = vec![0; 9 * 8 * 4];
    for y in 0..8 {
        for x in 0..9 {
            let i = (y * 9 + x) * 4;
            let value = if x < 4 && (2..6).contains(&y) {
                90
            } else if x >= 5 && (1..5).contains(&y) {
                220
            } else {
                0
            };
            pixels[i..i + 4].copy_from_slice(&[value, value, value, 255]);
            if x == 4 {
                pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 0]);
            }
        }
    }
    encode(9, 8, &pixels)
}
#[test]
fn png_halves_keep_orientation_align_tips_and_leave_the_center_empty() {
    let image = SplitBackdrop::from_png(&hands()).unwrap();
    let mut resolver = AsciiResolver::default();
    for (w, h, left, right, y) in [(20, 10, 6., 14., 5.), (80, 24, 32., 48., 12.)] {
        let mut grid = CharGrid::new(w, h);
        image.render(
            &mut grid,
            0.5,
            Contacts { left, right, y },
            1.0,
            &mut resolver,
        );
        assert!(grid.rows().flatten().all(|c| RAMP.contains(c)));
        assert!(
            grid.rows()
                .any(|row| row[..left as usize].iter().any(|&c| c != b' '))
        );
        assert!(
            grid.rows()
                .any(|row| row[right as usize..].iter().any(|&c| c != b' '))
        );
        assert!(grid.rows().all(|row| {
            row[left as usize..right as usize]
                .iter()
                .all(|&c| c == b' ')
        }));
        assert_eq!(grid.get(2, y as usize), Some(b'-'));
        assert_eq!(grid.get(w - 3, y as usize), Some(b'%'));
    }
}
#[test]
fn hand_scale_respects_physical_cell_aspect_and_zero_brightness() {
    let image = SplitBackdrop::from_png(&hands()).unwrap();
    let mut resolver = AsciiResolver::default();
    let mut heights = Vec::new();
    for aspect in [0.5, 1.0] {
        let mut grid = CharGrid::new(40, 40);
        image.render(
            &mut grid,
            aspect,
            Contacts {
                left: 12.,
                right: 28.,
                y: 20.,
            },
            1.0,
            &mut resolver,
        );
        heights.push(
            grid.rows()
                .filter(|row| row.iter().any(|&c| c != b' '))
                .count(),
        );
        image.render(
            &mut grid,
            aspect,
            Contacts {
                left: 12.,
                right: 28.,
                y: 20.,
            },
            0.0,
            &mut resolver,
        );
        assert!(grid.rows().flatten().all(|&c| c == b' '));
    }
    assert!(heights[1].abs_diff(2 * heights[0]) <= 2, "{heights:?}");
}
#[test]
fn transparent_or_missing_halves_and_invalid_pngs_are_errors() {
    assert!(SplitBackdrop::from_png(b"not PNG").is_err());
    let transparent = encode(
        4,
        4,
        &[255; 4 * 4 * 4]
            .chunks(4)
            .flat_map(|_| [255, 255, 255, 0])
            .collect::<Vec<_>>(),
    );
    assert!(SplitBackdrop::from_png(&transparent).is_err());
    let mut pixels = vec![0; 4 * 4 * 4];
    pixels[0..4].copy_from_slice(&[255, 255, 255, 255]);
    let missing = encode(4, 4, &pixels);
    assert!(SplitBackdrop::from_png(&missing).is_err());
}
#[test]
fn tiny_terminal_layouts_are_safe() {
    let image = SplitBackdrop::from_png(&hands()).unwrap();
    let mut resolver = AsciiResolver::default();
    for (w, h) in [(0, 0), (0, 8), (1, 1), (2, 4), (3, 1)] {
        let viewport = Viewport::centered(w, h, 0.42);
        assert!(viewport.x + viewport.width <= w);
        let frame = Framebuffer::new(viewport.width * 2, h * 4);
        let contacts = Contacts::from_frame(&frame, viewport, 2, 4, 1.0);
        let mut grid = CharGrid::new(w, h);
        image.render(&mut grid, 0.5, contacts, 0.65, &mut resolver);
        overlay_model(
            &mut grid,
            &CharGrid::new(viewport.width, h),
            &frame,
            viewport,
            2,
            4,
        );
    }
}
#[test]
fn dark_geometry_occludes_the_backdrop_and_empty_cells_preserve_it() {
    let viewport = Viewport {
        x: 1,
        width: 2,
        height: 1,
    };
    let mut output = CharGrid::new(4, 1);
    for x in 0..4 {
        output.set(x, 0, b'#');
    }
    let foreground = CharGrid::new(2, 1);
    let mut frame = Framebuffer::new(4, 4);
    frame.write(0, 0, 0.3, 0.0);
    overlay_model(&mut output, &foreground, &frame, viewport, 2, 4);
    assert_eq!(output.rows().next().unwrap(), b"# ##");
    let contacts = Contacts::from_frame(&frame, viewport, 2, 4, 1.0);
    assert!(contacts.left < 1.0 && contacts.right >= 2.0);
}
#[test]
fn static_tonal_lookup_preserves_model_history() {
    let mut r = AsciiResolver::default();
    let mut frame = Framebuffer::new(2, 4);
    let mut grid = CharGrid::new(1, 1);
    let options = AsciiOptions {
        glyph_mode: GlyphMode::Shape,
        palette: GlyphPalette::Tonal,
        tone: ToneMap::LINEAR,
        ..Default::default()
    };
    for y in 0..4 {
        for x in 0..2 {
            frame.write(x, y, 0.5, 0.499);
        }
    }
    r.resolve(&frame, 2, 4, &mut grid, options);
    assert_eq!(grid.get(0, 0), Some(b'='));
    for i in 0..=100 {
        r.tonal_char(i as f32 / 100.0);
    }
    frame.clear();
    for y in 0..4 {
        for x in 0..2 {
            frame.write(x, y, 0.5, 0.501);
        }
    }
    r.resolve(&frame, 2, 4, &mut grid, options);
    assert_eq!(grid.get(0, 0), Some(b'='));
}
