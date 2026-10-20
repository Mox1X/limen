use crate::font_atlas::GlyphAtlas;
pub use crate::glyphs::GlyphMode;
use crate::glyphs::match_glyph;
use crate::render::Framebuffer;
use crate::sampling::CellSampler;

pub const RAMP: &[u8] = b" .:-=+*#%@";

/// Applied to mean character luminance in brightness mode, or to each
/// resampled mask value in shape mode, before ASCII quantization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneMap {
    pub contrast: f32,
    /// Exponent: values greater than one darken midtones.
    pub gamma: f32,
}
impl ToneMap {
    pub const LINEAR: Self = Self {
        contrast: 1.0,
        gamma: 1.0,
    };
    pub fn apply(self, luminance: f32) -> f32 {
        (self.contrast * (luminance - 0.5) + 0.5)
            .clamp(0.0, 1.0)
            .powf(self.gamma)
    }
}
impl Default for ToneMap {
    fn default() -> Self {
        Self {
            contrast: 1.2,
            gamma: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AsciiOptions {
    pub glyph_mode: GlyphMode,
    pub tone: ToneMap,
}

#[derive(Debug, Clone)]
pub struct CharGrid {
    width: usize,
    height: usize,
    cells: Vec<u8>,
}

impl CharGrid {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![b' '; width * height],
        }
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn clear(&mut self) {
        self.cells.fill(b' ');
    }
    /// Only printable ASCII is accepted; out-of-bounds writes are ignored.
    pub fn set(&mut self, x: usize, y: usize, value: u8) {
        if x < self.width && y < self.height && (b' '..=b'~').contains(&value) {
            self.cells[y * self.width + x] = value;
        }
    }
    pub fn get(&self, x: usize, y: usize) -> Option<u8> {
        if x < self.width && y < self.height {
            Some(self.cells[y * self.width + x])
        } else {
            None
        }
    }
    pub fn rows(&self) -> impl Iterator<Item = &[u8]> {
        self.cells.chunks(self.width.max(1))
    }
}

pub fn brightness_char(value: f32) -> u8 {
    let index = (value.clamp(0.0, 1.0) * (RAMP.len() - 1) as f32).round() as usize;
    RAMP[index]
}

/// Background samples participate in the average, smoothing the silhouette too.
pub fn resolve(frame: &Framebuffer, samples_x: usize, samples_y: usize, grid: &mut CharGrid) {
    resolve_with_tone(frame, samples_x, samples_y, grid, ToneMap::LINEAR);
}

pub fn resolve_with_tone(
    frame: &Framebuffer,
    samples_x: usize,
    samples_y: usize,
    grid: &mut CharGrid,
    tone: ToneMap,
) {
    resolve_with_options(
        frame,
        samples_x,
        samples_y,
        grid,
        AsciiOptions {
            glyph_mode: GlyphMode::Brightness,
            tone,
        },
    );
}

pub fn resolve_with_options(
    frame: &Framebuffer,
    samples_x: usize,
    samples_y: usize,
    grid: &mut CharGrid,
    options: AsciiOptions,
) {
    AsciiResolver::default().resolve(frame, samples_x, samples_y, grid, options);
}

/// Owns an optional calibrated atlas and cached resampling weights.
#[derive(Default)]
pub struct AsciiResolver {
    atlas: Option<GlyphAtlas>,
    sampler: Option<(usize, usize, CellSampler)>,
}
impl AsciiResolver {
    pub fn with_atlas(atlas: GlyphAtlas) -> Self {
        Self {
            atlas: Some(atlas),
            sampler: None,
        }
    }
    pub fn atlas(&self) -> Option<&GlyphAtlas> {
        self.atlas.as_ref()
    }
    pub fn resolve(
        &mut self,
        frame: &Framebuffer,
        samples_x: usize,
        samples_y: usize,
        grid: &mut CharGrid,
        options: AsciiOptions,
    ) {
        if self
            .sampler
            .as_ref()
            .is_some_and(|(x, y, _)| (*x, *y) != (samples_x, samples_y))
        {
            self.sampler = None;
        }
        assert!(samples_x > 0 && samples_y > 0);
        assert_eq!(frame.width(), grid.width() * samples_x);
        assert_eq!(frame.height(), grid.height() * samples_y);
        if options.glyph_mode == GlyphMode::Shape {
            let (_, _, sampler) = self.sampler.get_or_insert_with(|| {
                (samples_x, samples_y, CellSampler::new(samples_x, samples_y))
            });
            for y in 0..grid.height() {
                for x in 0..grid.width() {
                    let (values, covered) = sampler.read(frame, x * samples_x, y * samples_y);
                    let pattern = std::array::from_fn(|i| {
                        if covered[i] {
                            options.tone.apply(values[i])
                        } else {
                            0.0
                        }
                    });
                    grid.set(
                        x,
                        y,
                        match &self.atlas {
                            Some(atlas) => atlas.select_shape(pattern),
                            None => match_glyph(pattern),
                        },
                    );
                }
            }
            return;
        }
        let tone = options.tone;
        let divisor = (samples_x * samples_y) as f32;
        for y in 0..grid.height() {
            for x in 0..grid.width() {
                let mut sum = 0.0;
                let mut covered = false;
                for sy in 0..samples_y {
                    for sx in 0..samples_x {
                        let index = (y * samples_y + sy) * frame.width() + x * samples_x + sx;
                        sum += frame.brightness()[index];
                        covered |= frame.depth()[index].is_finite();
                    }
                }
                grid.set(
                    x,
                    y,
                    if covered {
                        match &self.atlas {
                            Some(atlas) => atlas.select_brightness(tone.apply(sum / divisor)),
                            None => brightness_char(tone.apply(sum / divisor)),
                        }
                    } else {
                        b' '
                    },
                );
            }
        }
    }
}

impl CellSampler {
    fn read(&self, frame: &Framebuffer, x: usize, y: usize) -> ([f32; 8], [bool; 8]) {
        self.sample(|sx, sy| {
            let index = (y + sy) * frame.width() + x + sx;
            (frame.brightness()[index], frame.depth()[index].is_finite())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_bounds_and_clear() {
        let mut grid = CharGrid::new(2, 3);
        grid.set(1, 2, b'@');
        assert_eq!(grid.get(1, 2), Some(b'@'));
        grid.set(2, 0, b'@');
        grid.set(usize::MAX, usize::MAX, b'@');
        grid.set(0, 0, b'\n');
        assert_eq!(grid.get(0, 0), Some(b' '));
        assert_eq!(grid.get(2, 0), None);
        grid.clear();
        assert!(grid.rows().flatten().all(|&c| c == b' '));
    }
    #[test]
    fn fractional_area_resampling_preserves_luminance() {
        let mut frame = Framebuffer::new(3, 5);
        for y in 0..5 {
            for x in 0..3 {
                frame.write(x, y, 0.5, x as f32 / 2.0);
            }
        }
        let (values, covered) = CellSampler::new(3, 5).read(&frame, 0, 0);
        for row in 0..4 {
            assert!((values[row * 2] - 1.0 / 6.0).abs() < 1e-6);
            assert!((values[row * 2 + 1] - 5.0 / 6.0).abs() < 1e-6);
        }
        assert!(covered.into_iter().all(|v| v));
        for y in 0..5 {
            for x in 0..3 {
                frame.write(x, y, 0.25, y as f32 / 4.0);
            }
        }
        let (values, _) = CellSampler::new(3, 5).read(&frame, 0, 0);
        for (row, expected) in [0.05, 0.35, 0.65, 0.95].into_iter().enumerate() {
            assert!((values[row * 2] - expected).abs() < 1e-6);
            assert!((values[row * 2 + 1] - expected).abs() < 1e-6);
        }
        let mut pixel = Framebuffer::new(1, 1);
        pixel.write(0, 0, 0.5, 0.4);
        let (values, covered) = CellSampler::new(1, 1).read(&pixel, 0, 0);
        assert_eq!(values, [0.4; 8]);
        assert_eq!(covered, [true; 8]);
    }

    #[test]
    fn ramp_and_sample_average() {
        assert_eq!(brightness_char(-1.0), b' ');
        assert_eq!(brightness_char(2.0), b'@');
        let mut frame = Framebuffer::new(2, 4);
        frame.write(0, 0, 0.5, 1.0);
        let mut grid = CharGrid::new(1, 1);
        resolve(&frame, 2, 4, &mut grid);
        assert_eq!(grid.get(0, 0), Some(brightness_char(0.125)));
    }
}
