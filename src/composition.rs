//! Static split-image backdrops and terminal-space composition, independent of OBJ loading.
use crate::{
    ascii::{AsciiResolver, CharGrid, ToneMap},
    render::Framebuffer,
};
use std::{
    error::Error,
    fs::File,
    io::{BufReader, Cursor},
    path::Path,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub const DEFAULT_CENTER_WIDTH: f32 = 0.36;

/// Static image tone is independent of the model's lighting and exposure.
#[derive(Debug, Clone, Copy)]
pub struct BackdropStyle {
    pub brightness: f32,
    pub tone: ToneMap,
}
impl Default for BackdropStyle {
    fn default() -> Self {
        Self {
            brightness: 0.85,
            tone: ToneMap {
                contrast: 1.5,
                gamma: 1.1,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub x: usize,
    pub width: usize,
    pub height: usize,
}
impl Viewport {
    pub fn centered(width: usize, height: usize, fraction: f32) -> Self {
        let central =
            ((width as f32 * fraction).round() as usize).clamp(usize::from(width > 0), width);
        Self {
            x: (width - central) / 2,
            width: central,
            height,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Contacts {
    /// Display character coordinates; fractional values preserve sample precision.
    pub left: f32,
    pub right: f32,
    pub y: f32,
}
impl Contacts {
    /// Use coverage, including unlit geometry, to place fingertips beside the model.
    pub fn from_frame(
        frame: &Framebuffer,
        viewport: Viewport,
        sx: usize,
        sy: usize,
        gap: f32,
    ) -> Self {
        assert!(sx > 0 && sy > 0);
        assert_eq!(frame.width(), viewport.width * sx);
        assert_eq!(frame.height(), viewport.height * sy);
        let y = viewport.height as f32 * 0.5;
        let center = (frame.height() / 2) as isize;
        let mut left = frame.width();
        let mut right = 0;
        let mut found = false;
        // If the middle band is empty, use the overall silhouette instead.
        for band in [true, false] {
            for py in 0..frame.height() {
                if band && (py as isize - center).unsigned_abs() > sy {
                    continue;
                }
                for px in 0..frame.width() {
                    if frame.depth()[py * frame.width() + px].is_finite() {
                        left = left.min(px);
                        right = right.max(px + 1);
                        found = true;
                    }
                }
            }
            if found {
                break;
            }
        }
        if !found {
            left = frame.width() / 4;
            right = frame.width() * 3 / 4;
        }
        Self {
            left: viewport.x as f32 + left as f32 / sx as f32 - gap,
            right: viewport.x as f32 + right as f32 / sx as f32 + gap,
            y,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Tip {
    x: f32,
    y: f32,
}

pub struct SplitBackdrop {
    width: usize,
    height: usize,
    split: usize,
    pixels: Vec<u8>,
    tips: [Tip; 2],
}
impl SplitBackdrop {
    pub fn from_file(path: &Path) -> Result<Self> {
        let reader =
            BufReader::new(File::open(path).map_err(|e| format!("{}: {e}", path.display()))?);
        Self::decode(reader).map_err(|e| format!("{}: {e}", path.display()).into())
    }
    pub fn from_png(bytes: &[u8]) -> Result<Self> {
        Self::decode(Cursor::new(bytes))
    }
    fn decode(reader: impl std::io::BufRead + std::io::Seek) -> Result<Self> {
        let mut decoder = png::Decoder::new(reader);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info()?;
        let width = reader.info().width as usize;
        let height = reader.info().height as usize;
        if width < 2 || height == 0 || width.checked_mul(height).is_none_or(|n| n > 32_000_000) {
            return Err(
                "backdrop must be at least 2 pixels wide and at most 32 million pixels".into(),
            );
        }
        if reader.info().animation_control.is_some() {
            return Err("backdrop must be a static PNG".into());
        }
        let mut bytes = vec![
            0;
            reader
                .output_buffer_size()
                .ok_or("PNG dimensions overflow")?
        ];
        let info = reader.next_frame(&mut bytes)?;
        let channels = info.color_type.samples();
        let mut pixels = Vec::with_capacity(width * height);
        for pixel in bytes[..info.buffer_size()].chunks_exact(channels) {
            let (luminance, alpha) = match info.color_type {
                png::ColorType::Grayscale => (pixel[0] as f32, 1.0),
                png::ColorType::GrayscaleAlpha => (pixel[0] as f32, pixel[1] as f32 / 255.0),
                png::ColorType::Rgb => (
                    0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32,
                    1.0,
                ),
                png::ColorType::Rgba => (
                    0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32,
                    pixel[3] as f32 / 255.0,
                ),
                _ => return Err("unsupported PNG pixel format".into()),
            };
            pixels.push((luminance * alpha).round() as u8);
        }
        let split = width / 2;
        let mut image = Self {
            width,
            height,
            split,
            pixels,
            tips: [Tip { x: 0.0, y: 0.0 }; 2],
        };
        image.tips = [
            image.find_tip(0, split, true)?,
            image.find_tip(split, width, false)?,
        ];
        Ok(image)
    }
    fn find_tip(&self, start: usize, end: usize, left: bool) -> Result<Tip> {
        let mut extreme = None;
        for x in start..end {
            if (0..self.height).any(|y| self.pixels[y * self.width + x] > 8) {
                extreme = Some(match extreme {
                    None => x,
                    Some(previous) if left => x.max(previous),
                    Some(previous) => x.min(previous),
                });
            }
        }
        let extreme =
            extreme.ok_or("each half of the backdrop must contain visible non-black pixels")?;
        // Average the contour's inner strip to avoid a single antialiased pixel defining Y.
        let strip = ((end - start) / 100).max(1);
        let mut sum = 0.0;
        let mut weight = 0.0;
        for x in start..end {
            if x.abs_diff(extreme) > strip {
                continue;
            }
            for y in 0..self.height {
                let value = self.pixels[y * self.width + x];
                if value > 8 {
                    sum += (y as f32 + 0.5) * value as f32;
                    weight += value as f32;
                }
            }
        }
        Ok(Tip {
            x: extreme as f32 + if left { 1.0 } else { 0.0 },
            y: sum / weight,
        })
    }
    /// Box filter in source space. Pixels outside the chosen half remain black.
    fn average(&self, half: usize, rect: [f32; 4]) -> f32 {
        let [left, top, right, bottom] = rect;
        let area = (right - left) * (bottom - top);
        if !area.is_finite() || area <= 0.0 {
            return 0.0;
        }
        let (start, end) = if half == 0 {
            (0, self.split)
        } else {
            (self.split, self.width)
        };
        let l = left.max(start as f32);
        let r = right.min(end as f32);
        let t = top.max(0.0);
        let b = bottom.min(self.height as f32);
        if l >= r || t >= b {
            return 0.0;
        }
        let mut sum = 0.0;
        for y in t.floor() as usize..b.ceil() as usize {
            let wy = (b.min(y as f32 + 1.0) - t.max(y as f32)).max(0.0);
            for x in l.floor() as usize..r.ceil() as usize {
                let wx = (r.min(x as f32 + 1.0) - l.max(x as f32)).max(0.0);
                sum += self.pixels[y * self.width + x] as f32 * wx * wy;
            }
        }
        sum / (255.0 * area)
    }
    /// Both hands use one physical scale. Outer arm ends extend beyond the viewport
    /// as needed, while inner tips are anchored independently beside the model.
    pub fn render(
        &self,
        grid: &mut CharGrid,
        cell_aspect: f32,
        contacts: Contacts,
        brightness: f32,
        resolver: &mut AsciiResolver,
    ) {
        self.render_with_style(
            grid,
            cell_aspect,
            contacts,
            BackdropStyle {
                brightness,
                tone: ToneMap::LINEAR,
            },
            resolver,
        );
    }
    pub fn render_with_style(
        &self,
        grid: &mut CharGrid,
        cell_aspect: f32,
        contacts: Contacts,
        style: BackdropStyle,
        resolver: &mut AsciiResolver,
    ) {
        assert!(cell_aspect.is_finite() && cell_aspect > 0.0);
        assert!(style.brightness.is_finite() && (0.0..=1.0).contains(&style.brightness));
        assert!((1.0..=3.0).contains(&style.tone.contrast));
        assert!((0.25..=4.0).contains(&style.tone.gamma));
        grid.clear();
        if grid.width() == 0 || grid.height() == 0 {
            return;
        }
        let physical_width = grid.width() as f32 * cell_aspect;
        let anchors = [
            contacts.left.clamp(0.0, grid.width() as f32) * cell_aspect,
            contacts.right.clamp(0.0, grid.width() as f32) * cell_aspect,
        ];
        let scale = (anchors[0] / self.tips[0].x)
            .max((physical_width - anchors[1]) / (self.width as f32 - self.tips[1].x));
        if scale <= 0.0 {
            return;
        }
        let origins = self.tips.map(|tip| (tip.x, tip.y));
        for y in 0..grid.height() {
            for x in 0..grid.width() {
                let mut value = 0.0;
                for sy in 0..4 {
                    for sx in 0..2 {
                        let px = (x as f32 + sx as f32 / 2.0) * cell_aspect;
                        let py = y as f32 + sy as f32 / 4.0;
                        let mut sample = 0.0_f32;
                        for half in 0..2 {
                            let origin_x = anchors[half] - origins[half].0 * scale;
                            let origin_y = contacts.y - origins[half].1 * scale;
                            sample = sample.max(self.average(
                                half,
                                [
                                    (px - origin_x) / scale,
                                    (py - origin_y) / scale,
                                    (px + cell_aspect / 2.0 - origin_x) / scale,
                                    (py + 0.25 - origin_y) / scale,
                                ],
                            ));
                        }
                        value += sample;
                    }
                }
                let luminance = style.tone.apply(value / 8.0) * style.brightness;
                grid.set(x, y, resolver.tonal_char(luminance));
            }
        }
    }
}

/// Composite by geometric coverage, so a covered but black model cell still
/// occludes the photo. PNG luminance never participates in the model's Z-buffer.
pub fn overlay_model(
    output: &mut CharGrid,
    foreground: &CharGrid,
    frame: &Framebuffer,
    viewport: Viewport,
    sx: usize,
    sy: usize,
) {
    assert_eq!(
        (foreground.width(), foreground.height()),
        (viewport.width, viewport.height)
    );
    assert_eq!(
        (frame.width(), frame.height()),
        (viewport.width * sx, viewport.height * sy)
    );
    assert!(viewport.x + viewport.width <= output.width() && viewport.height <= output.height());
    for y in 0..viewport.height {
        for x in 0..viewport.width {
            let covered = (0..sy).any(|dy| {
                (0..sx).any(|dx| {
                    frame.depth()[(y * sy + dy) * frame.width() + x * sx + dx].is_finite()
                })
            });
            if covered {
                output.set(viewport.x + x, y, foreground.get(x, y).unwrap());
            }
        }
    }
}
