use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal,
};
use glam::{EulerRot, Quat};
use limen::{
    ascii::{self, CharGrid},
    geometry::{self, Camera, Scene},
    obj,
    render::{self, Framebuffer},
    terminal::Terminal,
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
    #[arg(long, default_value = "0.5", value_parser = parse_aspect)]
    cell_aspect: f32,
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
fn parse_aspect(s: &str) -> Result<f32, String> {
    let value: f32 = s.parse().map_err(|_| "expected a number".to_string())?;
    if value.is_finite() && (0.05..=5.0).contains(&value) {
        Ok(value)
    } else {
        Err("expected a finite value between 0.05 and 5".into())
    }
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
    // Validate assets before changing terminal state.
    let mesh = match args.model {
        Some(ref path) => obj::load_selected(path, &args.objects)
            .map_err(|e| format!("{}: {e}", path.display()))?,
        None => geometry::cube(),
    };
    if !io::stdout().is_terminal() || !io::stdin().is_terminal() {
        return Err("interactive terminal required (stdin and stdout must be TTYs)".into());
    }
    let mut scene = Scene::from_mesh(mesh);
    let camera = Camera::default();
    let mut terminal_output = Terminal::enter()?;
    let mut grid = CharGrid::new(0, 0);
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
        let (columns, rows) = terminal::size()?;
        let (width, height) = (columns as usize, rows as usize);
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
            frame = Framebuffer::new(width * sx, height * sy);
        }
        if width > 0 && height > 0 {
            render::render(
                &scene,
                &camera,
                width as f32 * args.cell_aspect / height as f32,
                &mut frame,
            );
            ascii::resolve(&frame, sx, sy, &mut grid);
            terminal_output.present(&grid)?;
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
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q' | 'Q') => return Ok(()),
                    KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Char(' ') if key.kind == KeyEventKind::Press => paused = !paused,
                    KeyCode::Char('r' | 'R') => angle = 0.0,
                    _ => {}
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
