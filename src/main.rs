use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal,
};
use glam::{EulerRot, Quat};
use limen::{
    ascii::{AsciiResolver, CharGrid, GlyphMode, GlyphPalette, ToneMap},
    composition::{Contacts, SplitBackdrop, Viewport, overlay_model},
    font_atlas::{CellSize, FontAtlasConfig, GlyphAtlas},
    geometry::{self, Camera, Scene},
    obj,
    render::{self, Framebuffer, RenderOptions},
    terminal::Terminal,
    view::{Adjustment, CellAspect, ViewMode, ViewSettings},
};
use std::{
    error::Error,
    io::{self, IsTerminal},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version, about = "CPU-rendered 3D models in monochrome ASCII")]
struct Args {
    /// OBJ geometry to display; defaults to a built-in cube
    model: Option<PathBuf>,
    /// List OBJ object/group names and exit without opening the terminal viewer
    #[arg(long, requires = "model", conflicts_with = "objects")]
    list_objects: bool,
    /// Render only this named OBJ object/group; repeat to include several
    #[arg(long = "object", requires = "model", value_name = "NAME")]
    objects: Vec<String>,
    /// PNG with a left and right hand on black/transparent background
    #[arg(long, value_name = "PNG")]
    hands: Option<PathBuf>,
    /// Fraction of terminal width reserved for the 3D model (default 0.42 with --hands)
    #[arg(long, requires = "hands", value_parser = parse_center_width)]
    center_width: Option<f32>,
    /// Fingertip distance from the initial silhouette in character columns (default 1)
    #[arg(long, requires = "hands", value_parser = parse_hands_gap)]
    hands_gap: Option<f32>,
    /// Static hand luminance multiplier (default 0.65)
    #[arg(long, requires = "hands", value_parser = parse_unit)]
    hands_brightness: Option<f32>,
    /// Initial rotation about the X axis, in degrees
    #[arg(long, default_value = "0", allow_hyphen_values = true, value_parser = parse_degrees)]
    rotate_x: f32,
    /// Initial rotation about the Y axis, in degrees
    #[arg(long, default_value = "0", allow_hyphen_values = true, value_parser = parse_degrees)]
    rotate_y: f32,
    /// Initial rotation about the Z axis, in degrees
    #[arg(long, default_value = "0", allow_hyphen_values = true, value_parser = parse_degrees)]
    rotate_z: f32,
    /// Maximum frames per second
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u16).range(1..=240))]
    fps: u16,
    /// Horizontal samples per character
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=16))]
    samples_x: u8,
    /// Vertical samples per character
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u8).range(1..=16))]
    samples_y: u8,
    /// Character width divided by character height
    #[arg(long, value_parser = parse_aspect)]
    cell_aspect: Option<f32>,
    /// Initial display mode: shaded, depth, silhouette, normal-x, normal-y, normal-z
    #[arg(long, default_value = "shaded")]
    view: ViewMode,
    /// ASCII selection algorithm: brightness, shape or geometry-aware hybrid
    #[arg(long, default_value = "hybrid")]
    glyph_mode: GlyphMode,
    /// Candidate repertoire: tonal, structural or full (requires --font)
    #[arg(long, default_value = "structural")]
    palette: GlyphPalette,
    /// Preference for simple symbols in shape/hybrid matching (0 disables)
    #[arg(long, default_value = "0.01", value_parser = parse_unit)]
    complexity_penalty: f32,
    /// Maximum error margin for retaining a similar previous cell (0 disables)
    #[arg(long, default_value = "0.005", value_parser = parse_unit)]
    stability: f32,
    /// TTF/OTF font used to calibrate glyph selection
    #[arg(long)]
    font: Option<PathBuf>,
    /// Font rasterization size in pixels per em (default 16 with --font)
    #[arg(long, requires = "font", value_parser = parse_font_size)]
    font_size: Option<f32>,
    /// Override atlas cell size WxH, dimensions 1 to 512
    #[arg(long, requires = "font")]
    font_cell: Option<CellSize>,
    /// Magnification relative to an automatically fitted camera (0.5 to 3)
    #[arg(long, default_value = "1", value_parser = parse_zoom)]
    zoom: f32,
    /// Light azimuth in degrees: 0 toward the camera, +90 toward +X
    #[arg(long, default_value = "-55", allow_hyphen_values = true, value_parser = parse_degrees)]
    light_azimuth: f32,
    /// Light elevation in degrees (-90 to 90)
    #[arg(long, default_value = "35", allow_hyphen_values = true, value_parser = parse_elevation)]
    light_elevation: f32,
    /// Ambient light contribution (0 to 1)
    #[arg(long, default_value = "0.06", value_parser = parse_ambient)]
    ambient: f32,
    /// Luminance contrast around 0.5 (0.25 to 3)
    #[arg(long, default_value = "1.2", value_parser = parse_contrast)]
    contrast: f32,
    /// Luminance exponent, greater than 1 darkens midtones (0.25 to 4)
    #[arg(long, default_value = "1", value_parser = parse_gamma)]
    gamma: f32,
}
fn parse_degrees(s: &str) -> Result<f32, String> {
    let value: f32 = s
        .parse()
        .map_err(|_| "expected degrees as a number".to_string())?;
    if value.is_finite() {
        Ok(value % 360.0)
    } else {
        Err("expected a finite angle in degrees".into())
    }
}
fn parse_bounded(s: &str, min: f32, max: f32) -> Result<f32, String> {
    let value: f32 = s.parse().map_err(|_| "expected a number".to_string())?;
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(format!("expected a finite value between {min} and {max}"))
    }
}
fn parse_center_width(s: &str) -> Result<f32, String> {
    parse_bounded(s, 0.2, 0.8)
}
fn parse_hands_gap(s: &str) -> Result<f32, String> {
    parse_bounded(s, 0.0, 10.0)
}

#[derive(Clone, Copy, PartialEq)]
struct BackdropKey {
    width: usize,
    height: usize,
    aspect: f32,
    zoom: f32,
}

fn parse_unit(s: &str) -> Result<f32, String> {
    parse_bounded(s, 0.0, 1.0)
}
fn parse_font_size(s: &str) -> Result<f32, String> {
    parse_bounded(s, 4.0, 128.0)
}
fn parse_aspect(s: &str) -> Result<f32, String> {
    parse_bounded(s, 0.05, 5.0)
}
fn parse_zoom(s: &str) -> Result<f32, String> {
    parse_bounded(s, 0.5, 3.0)
}
fn parse_elevation(s: &str) -> Result<f32, String> {
    parse_bounded(s, -90.0, 90.0)
}
fn parse_ambient(s: &str) -> Result<f32, String> {
    parse_bounded(s, 0.0, 1.0)
}
fn parse_contrast(s: &str) -> Result<f32, String> {
    parse_bounded(s, 0.25, 3.0)
}
fn parse_gamma(s: &str) -> Result<f32, String> {
    parse_bounded(s, 0.25, 4.0)
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if args.list_objects {
        let path = args.model.as_ref().expect("clap requires a model");
        for object in obj::list_objects(path).map_err(|e| format!("{}: {e}", path.display()))? {
            println!(
                "{}\t{} triangles",
                object.name.escape_default(),
                object.triangles
            );
        }
        return Ok(());
    }
    if args.palette == GlyphPalette::Full && args.font.is_none() {
        return Err("--palette full requires --font".into());
    }
    // Validate assets before changing terminal state.
    let mesh = match args.model {
        Some(ref path) => obj::load_selected(path, &args.objects)
            .map_err(|e| format!("{}: {e}", path.display()))?,
        None => geometry::cube(),
    };
    let backdrop = args
        .hands
        .as_deref()
        .map(SplitBackdrop::from_file)
        .transpose()?;
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
    let atlas_status = match resolver.atlas() {
        Some(atlas) => format!(
            "font:{}/{}px/{}",
            atlas
                .name()
                .chars()
                .filter(|c| c.is_ascii_graphic() || *c == ' ')
                .take(20)
                .collect::<String>(),
            atlas.size_px(),
            atlas.cell()
        ),
        None => "builtin".to_string(),
    };
    if !io::stdout().is_terminal() || !io::stdin().is_terminal() {
        return Err("interactive terminal required (stdin and stdout must be TTYs)".into());
    }
    let bounds = mesh.bounding_sphere().ok_or("model contains no vertices")?;
    let mut scene = Scene::from_mesh(mesh);
    let initial_settings = ViewSettings {
        mode: args.view,
        glyph_mode: args.glyph_mode,
        palette: args.palette,
        complexity_penalty: args.complexity_penalty,
        stability: args.stability,
        zoom: args.zoom,
        light_azimuth: args.light_azimuth,
        light_elevation: args.light_elevation,
        ambient: args.ambient,
        tone: ToneMap {
            contrast: args.contrast,
            gamma: args.gamma,
        },
    };
    let mut settings = initial_settings;
    let mut terminal_output = Terminal::enter()?;
    let mut grid = CharGrid::new(0, 0);
    let mut model_grid = CharGrid::new(0, 0);
    let mut background: Option<(BackdropKey, CharGrid)> = None;
    let mut frame = Framebuffer::new(0, 0);
    let sx = args.samples_x as usize;
    let sy = args.samples_y as usize;
    let frame_time = Duration::from_secs_f64(1.0 / args.fps as f64);
    let mut previous = Instant::now();
    let orientation = Quat::from_euler(
        EulerRot::XYZ,
        args.rotate_x.to_radians(),
        args.rotate_y.to_radians(),
        args.rotate_z.to_radians(),
    );
    let mut angle = 0.0_f32;
    let mut paused = false;
    loop {
        let start = Instant::now();
        let dt = start.duration_since(previous).as_secs_f32();
        previous = start;
        if !paused {
            angle = (angle + dt * 6.0_f32.to_radians()) % std::f32::consts::TAU;
        }
        scene.instances[0].transform.rotation = Quat::from_rotation_y(angle) * orientation;
        // Query pixels too: font changes can alter the cell ratio independently of columns.
        let size = terminal::window_size().or_else(|_| {
            terminal::size().map(|(columns, rows)| terminal::WindowSize {
                columns,
                rows,
                width: 0,
                height: 0,
            })
        })?;
        let cell_aspect = CellAspect::detect(
            args.cell_aspect,
            size.columns,
            size.rows,
            size.width,
            size.height,
        );
        let (width, height) = (size.columns as usize, size.rows.saturating_sub(1) as usize);
        let viewport = Viewport::centered(
            width,
            height,
            if backdrop.is_some() {
                args.center_width.unwrap_or(0.42)
            } else {
                1.0
            },
        );
        if grid.width() != width || grid.height() != height {
            let samples = width
                .checked_mul(sx)
                .and_then(|w| height.checked_mul(sy).and_then(|h| w.checked_mul(h)))
                .ok_or("frame dimensions overflow")?;
            if samples > 16_000_000 {
                return Err(
                    "frame exceeds 16 million samples; reduce sampling or terminal size".into(),
                );
            }
            grid = CharGrid::new(width, height);
            model_grid = CharGrid::new(viewport.width, height);
            frame = Framebuffer::new(viewport.width * sx, height * sy);
        }
        if width > 0 && height > 0 {
            let aspect = viewport.width as f32 * cell_aspect.value / height as f32;
            let camera = Camera::fitted(bounds, aspect, settings.zoom);
            scene.light = settings.light();
            let key = BackdropKey {
                width,
                height,
                aspect: cell_aspect.value,
                zoom: settings.zoom,
            };
            if let Some(image) = &backdrop
                && background.as_ref().is_none_or(|(old, _)| *old != key)
            {
                scene.instances[0].transform.rotation = orientation;
                render::render_with_options(
                    &scene,
                    &camera,
                    aspect,
                    &mut frame,
                    RenderOptions {
                        mode: ViewMode::Silhouette,
                        capture_features: false,
                    },
                );
                let contacts =
                    Contacts::from_frame(&frame, viewport, sx, sy, args.hands_gap.unwrap_or(1.0));
                let mut layer = CharGrid::new(width, height);
                image.render(
                    &mut layer,
                    cell_aspect.value,
                    contacts,
                    args.hands_brightness.unwrap_or(0.65),
                    &mut resolver,
                );
                background = Some((key, layer));
                scene.instances[0].transform.rotation = Quat::from_rotation_y(angle) * orientation;
            }
            render::render_with_options(
                &scene,
                &camera,
                aspect,
                &mut frame,
                RenderOptions {
                    mode: settings.mode,
                    capture_features: settings.glyph_mode == GlyphMode::Hybrid,
                },
            );
            resolver.resolve(&frame, sx, sy, &mut model_grid, settings.ascii_options());
            if settings.mode == ViewMode::Shaded
                && let Some((_, layer)) = &background
            {
                grid.copy_from(layer);
            } else {
                grid.clear();
            }
            overlay_model(&mut grid, &model_grid, &frame, viewport, sx, sy);
        }
        if width > 0 && size.rows > 0 {
            let status = format!(
                "{}{} z{:.2} c{:.2} g{:.2} L{:.0}/{:.0} A{:.3}({}) {}/{} {}{} | M P 1-6 +/- arrows ,. [] 0 Space R Q",
                settings.mode,
                if paused { " paused" } else { "" },
                settings.zoom,
                settings.tone.contrast,
                settings.tone.gamma,
                settings.light_azimuth,
                settings.light_elevation,
                cell_aspect.value,
                cell_aspect.source,
                settings.glyph_mode,
                settings.palette,
                atlas_status,
                if settings.mode == ViewMode::Shaded {
                    ""
                } else {
                    " debug:tonal"
                },
            );
            terminal_output.present_with_status(&grid, &status)?;
        }
        // Poll until the frame deadline, so input remains responsive while idle.
        let deadline = start + frame_time;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !event::poll(remaining)? {
                break;
            }
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                let old_options = settings.ascii_options();
                let old_light = (
                    settings.light_azimuth,
                    settings.light_elevation,
                    settings.ambient,
                );
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q' | 'Q') => return Ok(()),
                    KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Char(' ') if key.kind == KeyEventKind::Press => paused = !paused,
                    KeyCode::Char('r' | 'R') => {
                        angle = 0.0;
                        resolver.reset_history();
                    }
                    KeyCode::Char('1'..='6') => {
                        if let KeyCode::Char(number) = key.code {
                            settings.mode = ViewMode::ALL[(number as u8 - b'1') as usize];
                        }
                    }
                    KeyCode::Char('+' | '=') => settings.adjust(Adjustment::Zoom(1.1)),
                    KeyCode::Char('-') => settings.adjust(Adjustment::Zoom(1.0 / 1.1)),
                    KeyCode::Left => settings.adjust(Adjustment::Azimuth(-5.0)),
                    KeyCode::Right => settings.adjust(Adjustment::Azimuth(5.0)),
                    KeyCode::Up => settings.adjust(Adjustment::Elevation(5.0)),
                    KeyCode::Down => settings.adjust(Adjustment::Elevation(-5.0)),
                    KeyCode::Char(',') => settings.adjust(Adjustment::Contrast(-0.1)),
                    KeyCode::Char('.') => settings.adjust(Adjustment::Contrast(0.1)),
                    KeyCode::Char('[') => settings.adjust(Adjustment::Gamma(-0.1)),
                    KeyCode::Char(']') => settings.adjust(Adjustment::Gamma(0.1)),
                    KeyCode::Char('0') => settings = initial_settings,
                    KeyCode::Char('p' | 'P') => {
                        settings.palette = settings.palette.next(resolver.atlas().is_some());
                    }
                    KeyCode::Char('m' | 'M') => {
                        settings.glyph_mode = match settings.glyph_mode {
                            GlyphMode::Brightness => GlyphMode::Shape,
                            GlyphMode::Shape => GlyphMode::Hybrid,
                            GlyphMode::Hybrid => GlyphMode::Brightness,
                        }
                    }
                    _ => {}
                }
                if settings.ascii_options() != old_options
                    || (
                        settings.light_azimuth,
                        settings.light_elevation,
                        settings.ambient,
                    ) != old_light
                {
                    resolver.reset_history();
                }
            }
            if Instant::now() >= deadline {
                break;
            }
        }
    }
}
fn main() {
    // Report a panic only after stack unwinding has restored the terminal.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(run);
    std::panic::set_hook(original_hook);
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            eprintln!("limen: {error}");
            std::process::exit(1);
        }
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            eprintln!("limen: unexpected panic: {message}");
            std::process::exit(1);
        }
    }
}
