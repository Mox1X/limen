//! Compare the old and fitted views, optionally on a local OBJ and with text snapshots.
use clap::Parser;
use glam::{Quat, Vec3};
use limen::{
    ascii::{
        AsciiOptions, AsciiResolver, CharGrid, GlyphMode, GlyphPalette, ToneMap, resolve_with_tone,
    },
    font_atlas::{CellSize, FontAtlasConfig, GlyphAtlas},
    geometry::{Camera, DirectionalLight, Mesh, Scene, cube},
    glyphs::SHAPE_GLYPHS,
    obj,
    render::{Framebuffer, RenderOptions, render_with_options},
    view::ViewMode,
};
use std::{error::Error, hint::black_box, path::PathBuf, time::Instant};

#[derive(Parser)]
struct Args {
    model: Option<PathBuf>,
    #[arg(long = "object", requires = "model")]
    objects: Vec<String>,
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    rotate_x: f32,
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u32).range(1..=10000))]
    frames: u32,
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u16).range(8..=500))]
    columns: u16,
    /// Image rows, excluding the terminal application's footer
    #[arg(long, default_value_t = 40, value_parser = clap::value_parser!(u16).range(8..=200))]
    rows: u16,
    /// Write deterministic before/after and diagnostic snapshots of the OBJ
    #[arg(long)]
    snapshot_dir: Option<PathBuf>,
    /// Compare brightness and shape on the same fitted scene instead of old/new lighting
    #[arg(long)]
    compare_glyphs: bool,
    /// Rotation per glyph-comparison frame, radians (6 deg/s at 30 FPS)
    #[arg(long, default_value_t = 0.003490659_f32)]
    rotation_step: f32,
    #[arg(long, requires = "compare_glyphs")]
    font: Option<PathBuf>,
    #[arg(long, requires = "font", value_parser = parse_font_size)]
    font_size: Option<f32>,
    #[arg(long, requires = "font")]
    font_cell: Option<CellSize>,
}

fn parse_font_size(value: &str) -> Result<f32, String> {
    let size: f32 = value.parse().map_err(|_| "expected px/em".to_string())?;
    if size.is_finite() && (4.0..=128.0).contains(&size) {
        Ok(size)
    } else {
        Err("expected a finite size from 4 to 128 px/em".into())
    }
}
fn scene(mesh: Mesh, before: bool) -> Scene {
    let mut scene = Scene::from_mesh(mesh);
    if before {
        scene.light = DirectionalLight {
            toward_light: Vec3::new(-0.5, 0.8, 1.0).normalize(),
            ambient: 0.12,
            diffuse: 0.88,
        };
    }
    scene
}
fn text(grid: &CharGrid) -> Vec<u8> {
    let mut output = Vec::new();
    for row in grid.rows() {
        output.extend_from_slice(row.trim_ascii_end());
        output.push(b'\n');
    }
    output
}
fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if !args.rotate_x.is_finite() || !args.rotation_step.is_finite() {
        return Err("rotation must be finite".into());
    }
    let path = args.model.clone().unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/smooth.obj")
    });
    let obj = obj::load_selected(&path, &args.objects)?;
    if args.compare_glyphs {
        return compare_glyphs(&args, obj);
    }
    let (width, height) = (args.columns as usize, args.rows as usize);
    let aspect = width as f32 * 0.5 / height as f32;
    let orientation = Quat::from_rotation_x(args.rotate_x.to_radians());
    for (name, mesh) in [("cube", cube()), ("OBJ", obj)] {
        let triangles = mesh.indices.len();
        let bounds = mesh.bounding_sphere().unwrap();
        for before in [true, false] {
            let mut scene = scene(mesh.clone(), before);
            let camera = if before {
                Camera::default()
            } else {
                Camera::fitted(bounds, aspect, 1.0)
            };
            let mut frame = Framebuffer::new(width * 2, height * 4);
            let mut grid = CharGrid::new(width, height);
            let tone = if before {
                ToneMap::LINEAR
            } else {
                ToneMap::default()
            };
            let start = Instant::now();
            for i in 0..args.frames {
                scene.instances[0].transform.rotation =
                    Quat::from_rotation_y(i as f32 * 0.01) * orientation;
                render_with_options(
                    &scene,
                    &camera,
                    aspect,
                    &mut frame,
                    RenderOptions::default(),
                );
                resolve_with_tone(&frame, 2, 4, &mut grid, tone);
                black_box(&grid);
            }
            let milliseconds = start.elapsed().as_secs_f64() * 1000.0 / args.frames as f64;
            let variant = if before { "before" } else { "after" };
            println!(
                "{name}/{variant}: {triangles} triangles, {width}x{height} chars, 2x4 samples, {milliseconds:.3} ms/frame ({:.0} FPS), excluding terminal I/O",
                1000.0 / milliseconds
            );
            if name == "OBJ"
                && let Some(ref directory) = args.snapshot_dir
            {
                std::fs::create_dir_all(directory)?;
                scene.instances[0].transform.rotation = orientation;
                for mode in ViewMode::ALL {
                    if before && mode != ViewMode::Shaded {
                        continue;
                    }
                    render_with_options(
                        &scene,
                        &camera,
                        aspect,
                        &mut frame,
                        RenderOptions {
                            mode,
                            ..Default::default()
                        },
                    );
                    let tone = if mode == ViewMode::Shaded {
                        tone
                    } else {
                        ToneMap::LINEAR
                    };
                    resolve_with_tone(&frame, 2, 4, &mut grid, tone);
                    let filename = if mode == ViewMode::Shaded {
                        variant
                    } else {
                        mode.as_str()
                    };
                    std::fs::write(directory.join(format!("{filename}.txt")), text(&grid))?;
                }
            }
        }
    }
    Ok(())
}

fn compare_glyphs(args: &Args, mesh: Mesh) -> Result<(), Box<dyn Error>> {
    let build_start = Instant::now();
    let mut resolver = match args.font {
        Some(ref path) => AsciiResolver::with_atlas(GlyphAtlas::from_file(
            path,
            FontAtlasConfig {
                size_px: args.font_size.unwrap_or(16.0),
                cell: args.font_cell,
            },
        )?),
        None => AsciiResolver::default(),
    };
    let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;
    if let Some(atlas) = resolver.atlas() {
        println!(
            "atlas: {} {}px/em cell={} baseline={} max_density={:.6}, build {build_ms:.3} ms",
            atlas.name(),
            atlas.size_px(),
            atlas.cell(),
            atlas.baseline(),
            atlas.max_density()
        );
        if let Some(ref directory) = args.snapshot_dir {
            atlas.export(directory)?;
        }
    } else {
        println!("atlas: builtin");
    }
    let (width, height) = (args.columns as usize, args.rows as usize);
    let aspect = width as f32 * 0.5 / height as f32;
    let triangles = mesh.indices.len();
    let camera = Camera::fitted(mesh.bounding_sphere().unwrap(), aspect, 1.0);
    let mut scene = Scene::from_mesh(mesh);
    let orientation = Quat::from_rotation_x(args.rotate_x.to_radians());
    let mut frame = Framebuffer::new(width * 2, height * 4);
    let mut grid = CharGrid::new(width, height);
    let legacy_palette = if resolver.atlas().is_some() {
        GlyphPalette::Full
    } else {
        GlyphPalette::Structural
    };
    let mut configurations = vec![
        (
            "brightness",
            AsciiOptions {
                glyph_mode: GlyphMode::Brightness,
                palette: if resolver.atlas().is_some() {
                    GlyphPalette::Full
                } else {
                    GlyphPalette::Tonal
                },
                complexity_penalty: 0.0,
                stability: 0.0,
                ..Default::default()
            },
        ),
        (
            "shape",
            AsciiOptions {
                glyph_mode: GlyphMode::Shape,
                palette: legacy_palette,
                complexity_penalty: 0.0,
                stability: 0.0,
                ..Default::default()
            },
        ),
        (
            "brightness-tonal",
            AsciiOptions {
                glyph_mode: GlyphMode::Brightness,
                palette: GlyphPalette::Tonal,
                ..Default::default()
            },
        ),
        (
            "hybrid-tonal",
            AsciiOptions {
                palette: GlyphPalette::Tonal,
                ..Default::default()
            },
        ),
        (
            "hybrid-structural-unstable",
            AsciiOptions {
                stability: 0.0,
                ..Default::default()
            },
        ),
        ("hybrid-structural", AsciiOptions::default()),
    ];
    if resolver.atlas().is_some() {
        configurations.push((
            "shape-full-penalized",
            AsciiOptions {
                glyph_mode: GlyphMode::Shape,
                palette: GlyphPalette::Full,
                stability: 0.0,
                ..Default::default()
            },
        ));
        configurations.push((
            "hybrid-full",
            AsciiOptions {
                palette: GlyphPalette::Full,
                ..Default::default()
            },
        ));
    }
    for (name, options) in configurations {
        let glyph_mode = options.glyph_mode;
        let render_options = RenderOptions {
            capture_features: glyph_mode == GlyphMode::Hybrid,
            ..Default::default()
        };
        resolver.reset_history();
        let mut previous_chars = Vec::new();
        let mut changes = 0_usize;
        let mut raster_seconds = 0.0;
        let mut resolve_seconds = 0.0;
        for i in 0..args.frames {
            scene.instances[0].transform.rotation =
                Quat::from_rotation_y(i as f32 * args.rotation_step) * orientation;
            let start = Instant::now();
            render_with_options(&scene, &camera, aspect, &mut frame, render_options);
            raster_seconds += start.elapsed().as_secs_f64();
            let start = Instant::now();
            resolver.resolve(&frame, 2, 4, &mut grid, options);
            resolve_seconds += start.elapsed().as_secs_f64();
            let chars: Vec<_> = grid.rows().flatten().copied().collect();
            changes += chars
                .iter()
                .zip(&previous_chars)
                .filter(|(a, b)| a != b)
                .count();
            previous_chars = chars;
            black_box(&grid);
        }
        let total_ms = (raster_seconds + resolve_seconds) * 1000.0 / args.frames as f64;
        let raster_ms = raster_seconds * 1000.0 / args.frames as f64;
        let moving_resolve_ms = resolve_seconds * 1000.0 / args.frames as f64;
        resolver.reset_history();
        scene.instances[0].transform.rotation = orientation;
        render_with_options(&scene, &camera, aspect, &mut frame, render_options);
        let start = Instant::now();
        for _ in 0..args.frames {
            resolver.resolve(black_box(&frame), 2, 4, &mut grid, options);
            black_box(&grid);
        }
        let resolve_ms = start.elapsed().as_secs_f64() * 1000.0 / args.frames as f64;
        println!(
            "{name}: {triangles} triangles, {width}x{height} chars, 2x4 samples, total {total_ms:.3} ms/frame (raster {raster_ms:.3}, moving resolve {moving_resolve_ms:.3}), fixed resolve {resolve_ms:.3} ms/frame; changed characters={changes}, excluding terminal I/O and statistics"
        );
        if let Some(ref directory) = args.snapshot_dir {
            std::fs::create_dir_all(directory)?;
            std::fs::write(directory.join(format!("{name}.txt")), text(&grid))?;
            render_with_options(
                &scene,
                &camera,
                aspect,
                &mut frame,
                RenderOptions {
                    mode: ViewMode::Silhouette,
                    ..Default::default()
                },
            );
            resolver.resolve(
                &frame,
                2,
                4,
                &mut grid,
                AsciiOptions {
                    glyph_mode,
                    tone: ToneMap::LINEAR,
                    palette: if resolver.atlas().is_some() {
                        limen::glyphs::GlyphPalette::Full
                    } else if glyph_mode == GlyphMode::Brightness {
                        limen::glyphs::GlyphPalette::Tonal
                    } else {
                        limen::glyphs::GlyphPalette::Structural
                    },
                    complexity_penalty: 0.0,
                    stability: 0.0,
                },
            );
            std::fs::write(
                directory.join(format!("{name}-silhouette.txt")),
                text(&grid),
            )?;
        }
    }
    if let Some(ref directory) = args.snapshot_dir {
        let mut report = String::from(
            "Synthetic 2x4 inputs (builtin templates), linear tone; columns: label, mean, brightness, shape\n",
        );
        for (label, mask) in SHAPE_GLYPHS
            .iter()
            .map(|g| (format!("glyph {}", g.symbol as char), g.mask))
            .chain([
                ("uniform black".into(), [0.0; 8]),
                ("uniform gray".into(), [0.5; 8]),
                ("uniform white".into(), [1.0; 8]),
            ])
        {
            let mut frame = Framebuffer::new(2, 4);
            for (i, value) in mask.into_iter().enumerate() {
                frame.write(i % 2, i / 2, 0.5, value);
            }
            let mut cell = CharGrid::new(1, 1);
            let mut outputs = Vec::new();
            for glyph_mode in [GlyphMode::Brightness, GlyphMode::Shape] {
                resolver.resolve(
                    &frame,
                    2,
                    4,
                    &mut cell,
                    AsciiOptions {
                        glyph_mode,
                        tone: ToneMap::LINEAR,
                        palette: if resolver.atlas().is_some() {
                            limen::glyphs::GlyphPalette::Full
                        } else if glyph_mode == GlyphMode::Brightness {
                            limen::glyphs::GlyphPalette::Tonal
                        } else {
                            limen::glyphs::GlyphPalette::Structural
                        },
                        complexity_penalty: 0.0,
                        stability: 0.0,
                    },
                );
                outputs.push(cell.get(0, 0).unwrap() as char);
            }
            report.push_str(&format!(
                "{label:14} mean={:.3} brightness='{}' shape='{}' mask={mask:?}\n",
                mask.iter().sum::<f32>() / 8.0,
                outputs[0],
                outputs[1]
            ));
        }
        std::fs::write(directory.join("synthetic.txt"), report)?;
    }
    Ok(())
}
