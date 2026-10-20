//! Compare the old and fitted views, optionally on a local OBJ and with text snapshots.
use clap::Parser;
use glam::{Quat, Vec3};
use limen::{
    ascii::{CharGrid, ToneMap, resolve_with_tone},
    geometry::{Camera, DirectionalLight, Mesh, Scene, cube},
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
    if !args.rotate_x.is_finite() {
        return Err("rotation must be finite".into());
    }
    let path = args.model.unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/smooth.obj")
    });
    let obj = obj::load_selected(&path, &args.objects)?;
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
                        RenderOptions { mode },
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
