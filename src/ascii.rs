use crate::font_atlas::GlyphAtlas;
pub use crate::glyphs::{GlyphMode, GlyphPalette};
use crate::render::Framebuffer;
use crate::sampling::CellSampler;
use crate::selection::{Candidates, Cell, edge};
use crate::view::ViewMode;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AsciiOptions {
    pub glyph_mode: GlyphMode,
    pub tone: ToneMap,
    pub palette: GlyphPalette,
    pub complexity_penalty: f32,
    pub stability: f32,
}

impl Default for AsciiOptions {
    fn default() -> Self {
        Self {
            glyph_mode: GlyphMode::Hybrid,
            tone: ToneMap::default(),
            palette: GlyphPalette::Structural,
            complexity_penalty: 0.01,
            stability: 0.005,
        }
    }
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
            palette: GlyphPalette::Tonal,
            ..Default::default()
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

/// Owns the font atlas, palette caches and optional per-cell temporal history.
#[derive(Default)]
pub struct AsciiResolver {
    atlas: Option<GlyphAtlas>,
    sampler: Option<(usize, usize, CellSampler)>,
    candidates: Option<[Candidates; 3]>,
    history_key: Option<HistoryKey>,
    history: Vec<Option<(Cell, usize)>>,
    cells: Vec<Cell>,
}
#[derive(PartialEq)]
struct HistoryKey {
    dimensions: (usize, usize, usize, usize),
    options: AsciiOptions,
    projection: Option<(f32, f32)>,
    matrix: Option<glam::Mat4>,
    mode: ViewMode,
}
impl AsciiResolver {
    pub fn with_atlas(atlas: GlyphAtlas) -> Self {
        Self {
            atlas: Some(atlas),
            ..Self::default()
        }
    }
    pub fn atlas(&self) -> Option<&GlyphAtlas> {
        self.atlas.as_ref()
    }
    pub fn reset_history(&mut self) {
        self.history.fill(None);
        self.history_key = None;
    }
    pub fn resolve(
        &mut self,
        frame: &Framebuffer,
        samples_x: usize,
        samples_y: usize,
        grid: &mut CharGrid,
        mut options: AsciiOptions,
    ) {
        assert!(samples_x > 0 && samples_y > 0);
        assert_eq!(frame.width(), grid.width() * samples_x);
        assert_eq!(frame.height(), grid.height() * samples_y);
        assert!(
            options.palette != GlyphPalette::Full || self.atlas.is_some(),
            "full palette requires a font atlas"
        );
        assert!(options.complexity_penalty.is_finite() && options.complexity_penalty >= 0.0);
        assert!(options.stability.is_finite() && options.stability >= 0.0);
        if frame.view_mode() != ViewMode::Shaded {
            options = AsciiOptions {
                glyph_mode: GlyphMode::Brightness,
                palette: GlyphPalette::Tonal,
                tone: ToneMap::LINEAR,
                complexity_penalty: 0.0,
                stability: 0.0,
            };
        }
        let key = HistoryKey {
            dimensions: (grid.width(), grid.height(), samples_x, samples_y),
            options,
            projection: frame.clip_planes(),
            matrix: frame.view_projection(),
            mode: frame.view_mode(),
        };
        if self.history_key.as_ref() != Some(&key) {
            self.reset_history();
            self.history_key = Some(key);
        }
        if self
            .sampler
            .as_ref()
            .is_some_and(|(x, y, _)| (*x, *y) != (samples_x, samples_y))
        {
            self.sampler = None;
        }
        let sets = self.candidates.get_or_insert_with(|| {
            GlyphPalette::ALL.map(|p| Candidates::new(self.atlas.as_ref(), p))
        });
        let candidates = &sets[options.palette as usize];
        if options.glyph_mode == GlyphMode::Brightness {
            let divisor = (samples_x * samples_y) as f32;
            for y in 0..grid.height() {
                for x in 0..grid.width() {
                    let mut sum = 0.0;
                    let mut covered = false;
                    for sy in 0..samples_y {
                        for sx in 0..samples_x {
                            let i = (y * samples_y + sy) * frame.width() + x * samples_x + sx;
                            sum += frame.brightness()[i];
                            covered |= frame.depth()[i].is_finite();
                        }
                    }
                    let mean = options.tone.apply(sum / divisor);
                    // Preserve the legacy ramp's rounding at exact half steps.
                    let symbol = if !covered {
                        b' '
                    } else if self.atlas.is_none() && options.palette == GlyphPalette::Tonal {
                        brightness_char(mean)
                    } else if options.palette == GlyphPalette::Full {
                        self.atlas.as_ref().unwrap().select_brightness(mean)
                    } else {
                        candidates.brightness(mean)
                    };
                    grid.set(x, y, symbol);
                }
            }
            return;
        }
        let (_, _, sampler) = self
            .sampler
            .get_or_insert_with(|| (samples_x, samples_y, CellSampler::new(samples_x, samples_y)));
        let count = grid.width() * grid.height();
        self.cells.resize(count, Cell::default());
        self.history.resize(count, None);
        let geometry = options.glyph_mode == GlyphMode::Hybrid || options.stability > 0.0;
        for y in 0..grid.height() {
            for x in 0..grid.width() {
                self.cells[y * grid.width() + x] = Cell::read(
                    frame,
                    sampler,
                    x * samples_x,
                    y * samples_y,
                    options.tone,
                    geometry,
                );
            }
        }
        for y in 0..grid.height() {
            for x in 0..grid.width() {
                let i = y * grid.width() + x;
                let cell = self.cells[i];
                if cell.empty() {
                    grid.set(x, y, b' ');
                    self.history[i] = None;
                    continue;
                }
                if self.atlas.is_none()
                    && options.palette == GlyphPalette::Structural
                    && options.glyph_mode == GlyphMode::Shape
                    && options.complexity_penalty == 0.0
                    && options.stability == 0.0
                {
                    grid.set(x, y, crate::glyphs::match_glyph(cell.pattern));
                    continue;
                }
                // Explicitly preserve the previous full-atlas experimental algorithm.
                if options.palette == GlyphPalette::Full
                    && options.glyph_mode == GlyphMode::Shape
                    && options.complexity_penalty == 0.0
                    && options.stability == 0.0
                {
                    grid.set(
                        x,
                        y,
                        self.atlas.as_ref().unwrap().select_shape(cell.pattern),
                    );
                    continue;
                }
                let boundary = options.glyph_mode == GlyphMode::Hybrid
                    && edge(&self.cells, grid.width(), grid.height(), x, y);
                let chosen = candidates.choose(
                    cell.pattern,
                    options.glyph_mode,
                    boundary,
                    options.complexity_penalty,
                );
                let mut selected = chosen.0;
                if options.stability > 0.0
                    && let Some((previous, index)) = self.history[i]
                    && cell.similar(&previous)
                {
                    selected = candidates.retain(index, chosen, cell.pattern, boundary, options);
                }
                grid.set(x, y, candidates.symbol(selected));
                self.history[i] = Some((cell, selected));
            }
        }
    }
}

#[cfg(test)]
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
