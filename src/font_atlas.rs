//! Calibrated monochrome glyph coverage in a shared terminal cell.
use crate::sampling::CellSampler;
use fontdue::{Font, FontSettings};
use std::{error::Error, fmt, fs, path::Path, str::FromStr};

type AtlasResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellSize {
    pub width: usize,
    pub height: usize,
}
impl fmt::Display for CellSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}
impl FromStr for CellSize {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let error = "expected WxH with integer dimensions from 1 to 512";
        let (w, h) = value.split_once(['x', 'X']).ok_or(error)?;
        let cell = Self {
            width: w.parse().map_err(|_| error)?,
            height: h.parse().map_err(|_| error)?,
        };
        cell.validate().map_err(|e| e.to_string())?;
        Ok(cell)
    }
}
impl CellSize {
    fn validate(self) -> AtlasResult<()> {
        if !(1..=512).contains(&self.width) || !(1..=512).contains(&self.height) {
            return Err("font cell dimensions must be between 1 and 512".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FontAtlasConfig {
    pub size_px: f32,
    pub cell: Option<CellSize>,
}
impl Default for FontAtlasConfig {
    fn default() -> Self {
        Self {
            size_px: 16.0,
            cell: None,
        }
    }
}
impl FontAtlasConfig {
    fn validate(self) -> AtlasResult<()> {
        if !self.size_px.is_finite() || !(4.0..=128.0).contains(&self.size_px) {
            return Err("font size must be finite and between 4 and 128 px/em".into());
        }
        if let Some(cell) = self.cell {
            cell.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct FontGlyph {
    pub symbol: u8,
    pub mask: [f32; 8],
    pub density: f32,
    // Retained for inspection/export; matching only reads the mask and density.
    pixels: Vec<u8>,
}
impl FontGlyph {
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

#[derive(Debug)]
pub struct GlyphAtlas {
    name: String,
    size_px: f32,
    cell: CellSize,
    baseline: i32,
    origin_x: i32,
    glyphs: Vec<FontGlyph>,
    density_order: Vec<usize>,
    max_density: f32,
}
impl GlyphAtlas {
    pub fn from_file(path: &Path, config: FontAtlasConfig) -> AtlasResult<Self> {
        let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_bytes(&bytes, config).map_err(|e| format!("{}: {e}", path.display()).into())
    }
    pub fn from_bytes(bytes: &[u8], config: FontAtlasConfig) -> AtlasResult<Self> {
        config.validate()?;
        let font = Font::from_bytes(bytes, FontSettings::default())
            .map_err(|e| format!("cannot parse font: {e}"))?;
        let line = font
            .horizontal_line_metrics(config.size_px)
            .ok_or("font has no horizontal line metrics")?;
        if !line.ascent.is_finite()
            || !line.descent.is_finite()
            || !line.line_gap.is_finite()
            || !line.new_line_size.is_finite()
            || line.new_line_size <= 0.0
        {
            return Err("font has invalid line metrics".into());
        }
        for symbol in b' '..=b'~' {
            if !font.has_glyph(symbol as char) {
                return Err(format!("font is missing ASCII U+{symbol:04X}").into());
            }
        }
        let advance = font.metrics(' ', config.size_px).advance_width;
        if !advance.is_finite() || advance <= 0.0 {
            return Err("font has no positive space advance".into());
        }
        for symbol in b' '..=b'~' {
            let width = font.metrics(symbol as char, config.size_px).advance_width;
            if !width.is_finite() || (width - advance).abs() > 0.01 {
                return Err(format!(
                    "font must be monospaced: ASCII U+{symbol:04X} has a different advance"
                )
                .into());
            }
        }
        let cell = config.cell.unwrap_or(CellSize {
            width: advance.ceil() as usize,
            height: line.new_line_size.ceil() as usize,
        });
        cell.validate()?;
        let origin_x = ((cell.width as f32 - advance) * 0.5).round() as i32;
        let baseline =
            ((cell.height as f32 - line.new_line_size) * 0.5 + line.line_gap * 0.5 + line.ascent)
                .round() as i32;
        let sampler = CellSampler::new(cell.width, cell.height);
        let mut glyphs = Vec::with_capacity(95);
        for symbol in b' '..=b'~' {
            let (metrics, bitmap) = font.rasterize(symbol as char, config.size_px);
            let mut pixels = vec![0; cell.width * cell.height];
            let left = origin_x + metrics.xmin;
            let top = baseline - metrics.ymin - metrics.height as i32;
            blit(&mut pixels, cell, &bitmap, metrics.width, left, top);
            let (mask, _) =
                sampler.sample(|x, y| (pixels[y * cell.width + x] as f32 / 255.0, true));
            let density = mask.iter().sum::<f32>() / 8.0;
            glyphs.push(FontGlyph {
                symbol,
                mask,
                density,
                pixels,
            });
        }
        let mut density_order: Vec<_> = (0..glyphs.len()).collect();
        density_order.sort_by(|&a, &b| {
            glyphs[a]
                .density
                .total_cmp(&glyphs[b].density)
                .then(glyphs[a].symbol.cmp(&glyphs[b].symbol))
        });
        let max_density = glyphs[*density_order.last().unwrap()].density;
        if !max_density.is_finite() || max_density <= 0.0 {
            return Err("font atlas has no visible coverage in the selected cell".into());
        }
        Ok(Self {
            name: font.name().unwrap_or("Unnamed font").to_string(),
            size_px: config.size_px,
            cell,
            baseline,
            origin_x,
            glyphs,
            density_order,
            max_density,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn size_px(&self) -> f32 {
        self.size_px
    }
    pub fn cell(&self) -> CellSize {
        self.cell
    }
    pub fn baseline(&self) -> i32 {
        self.baseline
    }
    pub fn origin_x(&self) -> i32 {
        self.origin_x
    }
    pub fn glyphs(&self) -> &[FontGlyph] {
        &self.glyphs
    }
    pub fn max_density(&self) -> f32 {
        self.max_density
    }
    pub fn density_order(&self) -> impl Iterator<Item = &FontGlyph> {
        self.density_order.iter().map(|&i| &self.glyphs[i])
    }

    fn closest_density(&self, target: f32) -> usize {
        let at = self
            .density_order
            .partition_point(|&i| self.glyphs[i].density < target);
        let mut best = self.density_order[at.min(self.glyphs.len() - 1)];
        if at > 0 {
            let lower_density = self.glyphs[self.density_order[at - 1]].density;
            // Select the first ASCII character in a group of equal densities.
            let lower = self.density_order[self
                .density_order
                .partition_point(|&i| self.glyphs[i].density < lower_density)];
            let a = (self.glyphs[lower].density - target).abs();
            let b = (self.glyphs[best].density - target).abs();
            if a < b || (a == b && self.glyphs[lower].symbol < self.glyphs[best].symbol) {
                best = lower;
            }
        }
        best
    }
    /// Scene luminance 0..1 is mapped onto the fixed available density range.
    pub fn select_brightness(&self, luminance: f32) -> u8 {
        self.glyphs[self.closest_density(luminance.clamp(0.0, 1.0) * self.max_density)].symbol
    }
    /// Values are already tone mapped; coverage remains in the font's raw 0..1 range.
    pub fn select_shape(&self, pattern: [f32; 8]) -> u8 {
        let mean = pattern.iter().sum::<f32>() / 8.0;
        let best = self.closest_density(mean * self.max_density);
        let variance = pattern.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / 8.0;
        if variance <= 0.0025 {
            return self.glyphs[best].symbol;
        }
        self.match_coverage(pattern.map(|v| v * self.max_density), best)
    }
    fn match_coverage(&self, coverage: [f32; 8], mut best: usize) -> u8 {
        let error = |mask: [f32; 8]| {
            coverage
                .iter()
                .zip(mask)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
        };
        let mut best_error = error(self.glyphs[best].mask);
        for (i, glyph) in self.glyphs.iter().enumerate() {
            let candidate = error(glyph.mask);
            if candidate < best_error {
                best = i;
                best_error = candidate;
            }
        }
        self.glyphs[best].symbol
    }

    /// Inspection artifacts: unscaled cell bitmaps (16 columns) and mask/density TSV.
    pub fn export(&self, directory: &Path) -> std::io::Result<()> {
        fs::create_dir_all(directory)?;
        let stride_x = self.cell.width + 1;
        let stride_y = self.cell.height + 1;
        let width = 16 * stride_x;
        let height = self.glyphs.len().div_ceil(16) * stride_y;
        let mut image = vec![0; width * height];
        let mut table = String::from("code\tchar\tdensity\tm0\tm1\tm2\tm3\tm4\tm5\tm6\tm7\n");
        for (i, glyph) in self.glyphs.iter().enumerate() {
            for y in 0..self.cell.height {
                let start = (i / 16 * stride_y + y) * width + i % 16 * stride_x;
                image[start..start + self.cell.width]
                    .copy_from_slice(&glyph.pixels[y * self.cell.width..(y + 1) * self.cell.width]);
            }
            table.push_str(&format!(
                "U+{:04X}\t{}\t{:.8}",
                glyph.symbol, glyph.symbol as char, glyph.density
            ));
            for value in glyph.mask {
                table.push_str(&format!("\t{value:.8}"));
            }
            table.push('\n');
        }
        let mut pgm = format!("P5\n{width} {height}\n255\n").into_bytes();
        pgm.extend(image);
        fs::write(directory.join("atlas.pgm"), pgm)?;
        fs::write(directory.join("densities.tsv"), table)?;
        fs::write(
            directory.join("atlas-info.txt"),
            format!(
                "font={}\nsize_px={}\ncell={}\nbaseline={}\norigin_x={}\nglyphs={}\nmax_density={:.8}\ndensity_order={}\n",
                self.name,
                self.size_px,
                self.cell,
                self.baseline,
                self.origin_x,
                self.glyphs.len(),
                self.max_density,
                self.density_order()
                    .map(|g| g.symbol as char)
                    .collect::<String>()
            ),
        )
    }
}

fn blit(
    cell_pixels: &mut [u8],
    cell: CellSize,
    bitmap: &[u8],
    bitmap_width: usize,
    left: i32,
    top: i32,
) {
    if bitmap_width == 0 {
        return;
    }
    for (i, &value) in bitmap.iter().enumerate() {
        let x = left + (i % bitmap_width) as i32;
        let y = top + (i / bitmap_width) as i32;
        if x >= 0 && y >= 0 && (x as usize) < cell.width && (y as usize) < cell.height {
            cell_pixels[y as usize * cell.width + x as usize] = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_preserves_offsets_and_clips_without_rescaling() {
        let mut cell = vec![0; 16];
        blit(
            &mut cell,
            CellSize {
                width: 4,
                height: 4,
            },
            &[10, 20, 30, 40, 50, 60],
            3,
            -1,
            1,
        );
        assert_eq!(cell, [0, 0, 0, 0, 20, 30, 0, 0, 50, 60, 0, 0, 0, 0, 0, 0]);
    }
    #[test]
    fn duplicate_masks_use_density_candidate_then_ascii_order() {
        let fixture = include_bytes!("../tests/fixtures/fonts/JetBrainsMono-Regular.ttf");
        let mut atlas = GlyphAtlas::from_bytes(fixture, FontAtlasConfig::default()).unwrap();
        let mask = atlas.glyphs[15].mask;
        atlas.glyphs[16].mask = mask;
        assert_eq!(atlas.match_coverage(mask, 16), atlas.glyphs[16].symbol);
        assert_eq!(atlas.match_coverage(mask, 0), atlas.glyphs[15].symbol);
    }
}
