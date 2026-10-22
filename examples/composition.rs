//! Headless split-backdrop preview using the same compositor as the terminal viewer.
use clap::Parser;
use glam::Quat;
use limen::{
    ascii::{AsciiOptions, AsciiResolver, CharGrid, ToneMap},
    composition::{
        BackdropStyle, Contacts, DEFAULT_CENTER_WIDTH, SplitBackdrop, Viewport, overlay_model,
    },
    font_atlas::{FontAtlasConfig, GlyphAtlas},
    geometry::{self, Camera, Scene},
    obj,
    render::{Framebuffer, RenderOptions, render_with_options},
    view::ViewMode,
};
use std::{error::Error, io::Write, path::PathBuf};
#[derive(Parser)]
struct Args {
    model: Option<PathBuf>,
    #[arg(long = "object", requires = "model")]
    objects: Vec<String>,
    #[arg(long)]
    hands: PathBuf,
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    rotate_x: f32,
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    angle: f32,
    #[arg(long, default_value_t=120, value_parser=clap::value_parser!(u16).range(4..=500))]
    columns: u16,
    #[arg(long, default_value_t=40, value_parser=clap::value_parser!(u16).range(4..=200))]
    rows: u16,
    #[arg(long, default_value_t = 0.5)]
    cell_aspect: f32,
    #[arg(long, default_value_t = DEFAULT_CENTER_WIDTH)]
    center_width: f32,
    #[arg(long, default_value_t = 1.0)]
    hands_gap: f32,
    #[arg(long, default_value_t = 0.85)]
    hands_brightness: f32,
    #[arg(long, default_value_t = 1.5)]
    hands_contrast: f32,
    #[arg(long, default_value_t = 1.1)]
    hands_gamma: f32,
    #[arg(long)]
    font: Option<PathBuf>,
    #[arg(long)]
    snapshot_dir: Option<PathBuf>,
}
fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if !args.rotate_x.is_finite()
        || !args.angle.is_finite()
        || !args.cell_aspect.is_finite()
        || !(0.05..=5.0).contains(&args.cell_aspect)
        || !(0.2..=0.8).contains(&args.center_width)
        || !(0.0..=10.0).contains(&args.hands_gap)
        || !(0.0..=1.0).contains(&args.hands_brightness)
        || !(1.0..=3.0).contains(&args.hands_contrast)
        || !(0.25..=4.0).contains(&args.hands_gamma)
    {
        return Err("invalid composition parameters".into());
    }
    let mesh = match args.model {
        Some(ref path) => obj::load_selected(path, &args.objects)?,
        None => geometry::cube(),
    };
    let backdrop = SplitBackdrop::from_file(&args.hands)?;
    let mut resolver = match args.font {
        Some(ref path) => {
            AsciiResolver::with_atlas(GlyphAtlas::from_file(path, FontAtlasConfig::default())?)
        }
        None => AsciiResolver::default(),
    };
    let (w, h) = (args.columns as usize, args.rows as usize);
    let viewport = Viewport::centered(w, h, args.center_width);
    let aspect = viewport.width as f32 * args.cell_aspect / h as f32;
    let camera = Camera::fitted(mesh.bounding_sphere().ok_or("empty model")?, aspect, 1.0);
    let mut scene = Scene::from_mesh(mesh);
    let orientation = Quat::from_rotation_x(args.rotate_x.to_radians());
    scene.instances[0].transform.rotation = orientation;
    let mut frame = Framebuffer::new(viewport.width * 2, h * 4);
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
    let contacts = Contacts::from_frame(&frame, viewport, 2, 4, args.hands_gap);
    let mut output = CharGrid::new(w, h);
    backdrop.render_with_style(
        &mut output,
        args.cell_aspect,
        contacts,
        BackdropStyle {
            brightness: args.hands_brightness,
            tone: ToneMap {
                contrast: args.hands_contrast,
                gamma: args.hands_gamma,
            },
        },
        &mut resolver,
    );
    scene.instances[0].transform.rotation =
        Quat::from_rotation_y(args.angle.to_radians()) * orientation;
    render_with_options(
        &scene,
        &camera,
        aspect,
        &mut frame,
        RenderOptions {
            capture_features: true,
            ..Default::default()
        },
    );
    let mut foreground = CharGrid::new(viewport.width, h);
    resolver.resolve(&frame, 2, 4, &mut foreground, AsciiOptions::default());
    overlay_model(&mut output, &foreground, &frame, viewport, 2, 4);
    let mut text = Vec::new();
    for row in output.rows() {
        text.extend_from_slice(row.trim_ascii_end());
        text.push(b'\n');
    }
    if let Some(dir) = args.snapshot_dir {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("composition.txt"), text)?;
        if let Some(atlas) = resolver.atlas() {
            atlas.export(&dir)?;
        }
    } else {
        std::io::stdout().write_all(&text)?;
    }
    Ok(())
}
