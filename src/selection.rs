//! Palette-restricted matching and conservative geometry-aware edge detection.
use crate::{
    ascii::{AsciiOptions, RAMP, ToneMap},
    font_atlas::GlyphAtlas,
    glyphs::{GlyphMode, GlyphPalette, SHAPE_GLYPHS},
    render::Framebuffer,
    sampling::CellSampler,
};
use glam::Vec3;

#[derive(Clone)]
pub(crate) struct Candidate {
    pub symbol: u8,
    mask: [f32; 8],
    density: f32,
    complexity: f32,
    tonal: bool,
}
pub(crate) struct Candidates {
    glyphs: Vec<Candidate>,
    density_order: Vec<usize>,
    tonal_order: Vec<usize>,
}
impl Candidates {
    pub fn new(atlas: Option<&GlyphAtlas>, palette: GlyphPalette) -> Self {
        let mut glyphs = Vec::new();
        if let Some(atlas) = atlas {
            let max = atlas
                .glyphs()
                .iter()
                .filter(|g| palette.contains(g.symbol))
                .map(|g| g.density)
                .fold(0.0_f32, f32::max)
                .max(f32::EPSILON);
            for g in atlas.glyphs().iter().filter(|g| palette.contains(g.symbol)) {
                glyphs.push(Candidate::new(g.symbol, g.mask.map(|v| v / max)));
            }
        } else {
            for (i, &symbol) in RAMP.iter().enumerate() {
                glyphs.push(Candidate::new(
                    symbol,
                    [i as f32 / (RAMP.len() - 1) as f32; 8],
                ));
            }
            if palette != GlyphPalette::Tonal {
                glyphs.extend(
                    SHAPE_GLYPHS
                        .iter()
                        .map(|g| Candidate::new(g.symbol, g.mask)),
                );
            }
        }
        let mut density_order: Vec<_> = (0..glyphs.len()).collect();
        density_order.sort_by(|&a, &b| {
            glyphs[a]
                .density
                .total_cmp(&glyphs[b].density)
                .then(glyphs[a].symbol.cmp(&glyphs[b].symbol))
        });
        // Builtin ramp masks are uniform; the additional '-' stroke is not a ramp step.
        let tonal_order = density_order
            .iter()
            .copied()
            .filter(|&i| glyphs[i].tonal && (atlas.is_some() || i < RAMP.len()))
            .collect();
        Self {
            glyphs,
            density_order,
            tonal_order,
        }
    }
    fn nearest(&self, mean: f32, tonal: bool) -> usize {
        let order = if tonal {
            &self.tonal_order
        } else {
            &self.density_order
        };
        let at = order.partition_point(|&i| self.glyphs[i].density < mean);
        let mut best = order[at.min(order.len() - 1)];
        if at > 0 {
            let lower_density = self.glyphs[order[at - 1]].density;
            let lower = order[order.partition_point(|&i| self.glyphs[i].density < lower_density)];
            let a = (self.glyphs[lower].density - mean).abs();
            let b = (self.glyphs[best].density - mean).abs();
            if a < b || (a == b && self.glyphs[lower].symbol < self.glyphs[best].symbol) {
                best = lower;
            }
        }
        best
    }
    pub fn brightness(&self, mean: f32) -> u8 {
        self.glyphs[self.nearest(mean, false)].symbol
    }
    pub fn symbol(&self, index: usize) -> u8 {
        self.glyphs[index].symbol
    }
    pub fn score(&self, index: usize, pattern: [f32; 8], penalty: f32) -> f32 {
        let g = &self.glyphs[index];
        pattern
            .iter()
            .zip(g.mask)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            / 8.0
            + penalty * g.complexity
    }
    fn density_score(&self, index: usize, mean: f32) -> f32 {
        (self.glyphs[index].density - mean).powi(2)
    }
    fn allowed(&self, index: usize, hybrid: bool, edge: bool) -> bool {
        !hybrid || edge || self.glyphs[index].tonal
    }
    pub fn choose(
        &self,
        pattern: [f32; 8],
        mode: GlyphMode,
        edge: bool,
        penalty: f32,
    ) -> (usize, f32) {
        let mean = pattern.iter().sum::<f32>() / 8.0;
        let hybrid = mode == GlyphMode::Hybrid;
        let base = self.nearest(mean, hybrid);
        let base_error = self.score(base, pattern, penalty);
        let variance = pattern.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / 8.0;
        if (hybrid && !edge) || variance <= 0.0025 {
            return (base, self.density_score(base, mean));
        }
        let mut best = base;
        let mut error = base_error;
        for i in 0..self.glyphs.len() {
            if self.allowed(i, hybrid, edge) {
                let e = self.score(i, pattern, penalty);
                if e < error {
                    best = i;
                    error = e;
                }
            }
        }
        if hybrid && best != base && base_error - error < (base_error * 0.1).max(0.002) {
            (base, base_error)
        } else {
            (best, error)
        }
    }
    pub fn retain(
        &self,
        previous: usize,
        chosen: (usize, f32),
        pattern: [f32; 8],
        edge: bool,
        options: AsciiOptions,
    ) -> usize {
        let hybrid = options.glyph_mode == GlyphMode::Hybrid;
        let penalty = options.complexity_penalty;
        if !self.allowed(previous, hybrid, edge) {
            return chosen.0;
        }
        let mean = pattern.iter().sum::<f32>() / 8.0;
        let variance = pattern.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / 8.0;
        if hybrid && variance <= 0.0025 && !self.glyphs[previous].tonal {
            return chosen.0;
        }
        // Tone-only decisions need tone-only hysteresis. Comparing actual stroke
        // masks here can keep a lower-density glyph long after a ramp transition.
        if (hybrid && !edge) || variance <= 0.0025 {
            return if self.density_score(previous, mean) <= chosen.1 + options.stability {
                previous
            } else {
                chosen.0
            };
        }
        if hybrid && !self.glyphs[previous].tonal {
            let base_error = self.score(self.nearest(mean, true), pattern, penalty);
            let e = self.score(previous, pattern, penalty);
            if base_error - e < (base_error * 0.1).max(0.002) {
                return chosen.0;
            }
        }
        if self.score(previous, pattern, penalty) <= chosen.1 + options.stability {
            previous
        } else {
            chosen.0
        }
    }
}
impl Candidate {
    fn new(symbol: u8, mask: [f32; 8]) -> Self {
        let tonal = GlyphPalette::Tonal.contains(symbol);
        let complexity = if tonal {
            0.0
        } else if GlyphPalette::Structural.contains(symbol) {
            0.25
        } else if symbol.is_ascii_alphanumeric() {
            1.0
        } else {
            0.5
        };
        Self {
            symbol,
            mask,
            density: mask.iter().sum::<f32>() / 8.0,
            complexity,
            tonal,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Cell {
    pub pattern: [f32; 8],
    pub coverage: [f32; 8],
    inverse_depth: [f32; 8],
    normals: [Vec3; 8],
}
impl Cell {
    pub fn read(
        frame: &Framebuffer,
        sampler: &CellSampler,
        x: usize,
        y: usize,
        tone: ToneMap,
        geometry: bool,
    ) -> Self {
        let index = |sx, sy| (y + sy) * frame.width() + x + sx;
        let (values, covered) = sampler.sample(|sx, sy| {
            let i = index(sx, sy);
            (frame.brightness()[i], frame.depth()[i].is_finite())
        });
        let (coverage, _) =
            sampler.sample(|sx, sy| (f32::from(frame.depth()[index(sx, sy)].is_finite()), true));
        let mut cell = Self {
            pattern: std::array::from_fn(|i| {
                if covered[i] {
                    tone.apply(values[i])
                } else {
                    0.0
                }
            }),
            coverage,
            ..Self::default()
        };
        if geometry {
            if let Some((near, far)) = frame.clip_planes() {
                let (inverse, _) = sampler.sample(|sx, sy| {
                    let z = frame.depth()[index(sx, sy)];
                    (
                        if z.is_finite() {
                            (far - z * (far - near)) / (near * far)
                        } else {
                            0.0
                        },
                        true,
                    )
                });
                cell.inverse_depth = std::array::from_fn(|i| {
                    if coverage[i] > 0.0 {
                        inverse[i] / coverage[i]
                    } else {
                        0.0
                    }
                });
            }
            if let Some(normals) = frame.normals() {
                for axis in [0, 1, 2] {
                    let (component, _) =
                        sampler.sample(|sx, sy| (normals[index(sx, sy)][axis], true));
                    for (i, v) in component.into_iter().enumerate() {
                        cell.normals[i][axis] = v;
                    }
                }
                for n in &mut cell.normals {
                    *n = n.normalize_or_zero();
                }
            }
        }
        cell
    }
    pub fn empty(&self) -> bool {
        self.coverage.iter().all(|&c| c == 0.0)
    }
    pub fn similar(&self, previous: &Self) -> bool {
        for i in 0..8 {
            if (self.coverage[i] > 0.0) != (previous.coverage[i] > 0.0)
                || (self.coverage[i] - previous.coverage[i]).abs() > 0.01
            {
                return false;
            }
            let (a, b) = (self.inverse_depth[i], previous.inverse_depth[i]);
            if (a - b).abs() > 0.02 * a.abs().max(b.abs()) {
                return false;
            }
            let (a, b) = (self.normals[i], previous.normals[i]);
            if a != Vec3::ZERO && b != Vec3::ZERO && a.dot(b) < 5.0_f32.to_radians().cos() {
                return false;
            }
        }
        self.pattern
            .iter()
            .zip(previous.pattern)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            / 8.0
            <= 0.03_f32.powi(2)
    }
}

/// Evaluate screen-space differences at the 2x4 sample lattice, including neighbours.
/// Inverse view depth is affine across a projected plane: its second difference
/// avoids classifying an ordinary sloping plane as a depth discontinuity.
pub(crate) fn edge(cells: &[Cell], width: usize, height: usize, x: usize, y: usize) -> bool {
    let cell = &cells[y * width + x];
    if cell.coverage.iter().any(|&c| c > 1e-5 && c < 1.0 - 1e-5) || cell.coverage.contains(&0.0) {
        return !cell.empty();
    }
    let read = |px: isize, py: isize| -> Option<(&Cell, usize)> {
        if px < 0 || py < 0 || px >= (width * 2) as isize || py >= (height * 4) as isize {
            return None;
        }
        let (px, py) = (px as usize, py as usize);
        Some((&cells[(py / 4) * width + px / 2], (py % 4) * 2 + px % 2))
    };
    for i in 0..8 {
        let px = (x * 2 + i % 2) as isize;
        let py = (y * 4 + i / 2) as isize;
        for (dx, dy) in [(1, 0), (0, 1)] {
            let left = read(px - dx, py - dy);
            let right = read(px + dx, py + dy);
            for (c, j) in [left, right].into_iter().flatten() {
                if c.coverage[j] == 0.0 {
                    return true;
                }
                if cell.normals[i] != Vec3::ZERO
                    && c.normals[j] != Vec3::ZERO
                    && cell.normals[i].dot(c.normals[j]) < 25.0_f32.to_radians().cos()
                {
                    return true;
                }
            }
            if let (Some((a, j)), Some((b, k))) = (left, right) {
                if (a.pattern[j] - 2.0 * cell.pattern[i] + b.pattern[k]).abs() >= 0.15 {
                    return true;
                }
                let center = cell.inverse_depth[i];
                if center > 0.0
                    && a.inverse_depth[j] > 0.0
                    && b.inverse_depth[k] > 0.0
                    && (a.inverse_depth[j] - 2.0 * center + b.inverse_depth[k]).abs() / center
                        >= 0.05
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    fn smooth() -> Vec<Cell> {
        (0..9)
            .map(|index| {
                let (x, y) = (index % 3, index / 3);
                Cell {
                    coverage: [1.0; 8],
                    pattern: std::array::from_fn(|i| {
                        0.2 + (x * 2 + i % 2) as f32 * 0.02 + (y * 4 + i / 2) as f32 * 0.01
                    }),
                    inverse_depth: std::array::from_fn(|i| {
                        10.0 + (x * 2 + i % 2) as f32 * 0.1 + (y * 4 + i / 2) as f32 * 0.05
                    }),
                    normals: std::array::from_fn(|i| {
                        Vec3::new(
                            (x * 2 + i % 2) as f32 * 0.01,
                            (y * 4 + i / 2) as f32 * 0.01,
                            1.0,
                        )
                        .normalize()
                    }),
                }
            })
            .collect()
    }
    #[test]
    fn temporal_comparison_rejects_changed_coverage_depth_normals_and_brightness() {
        let original = smooth()[4];
        assert!(original.similar(&original));
        for feature in 0..4 {
            let mut changed = original;
            match feature {
                0 => changed.coverage[0] = 0.0,
                1 => changed.inverse_depth[0] *= 1.03,
                2 => changed.normals[0] = Vec3::Y,
                _ => changed.pattern = original.pattern.map(|v| v + 0.04),
            }
            assert!(!changed.similar(&original), "feature {feature}");
        }
    }
    #[test]
    fn smooth_light_depth_slope_and_gentle_normal_changes_are_not_edges() {
        assert!(!edge(&smooth(), 3, 3, 1, 1));
    }
    #[test]
    fn silhouette_brightness_depth_and_normal_discontinuities_open_candidates() {
        for feature in 0..4 {
            let mut cells = smooth();
            match feature {
                0 => cells[5].coverage = [0.0; 8],
                1 => cells[5].pattern = [1.0; 8],
                2 => cells[5].inverse_depth = [20.0; 8],
                _ => cells[5].normals = [Vec3::Y; 8],
            }
            assert!(edge(&cells, 3, 3, 1, 1), "feature {feature}");
        }
        let mut cells = smooth();
        cells[4].coverage[0] = 0.4;
        assert!(edge(&cells, 3, 3, 1, 1));
    }
    #[test]
    fn hybrid_requires_an_edge_and_a_clear_improvement() {
        let candidates = Candidates::new(None, GlyphPalette::Structural);
        let diagonal = SHAPE_GLYPHS[0].mask;
        let flat = candidates.choose(diagonal, GlyphMode::Hybrid, false, 0.01);
        assert!(GlyphPalette::Tonal.contains(candidates.symbol(flat.0)));
        assert_eq!(
            candidates.symbol(candidates.choose(diagonal, GlyphMode::Hybrid, true, 0.01).0),
            b'/'
        );
        let near_uniform = diagonal.map(|v| 0.5 + (v - 0.5) * 0.05);
        assert!(
            GlyphPalette::Tonal.contains(
                candidates.symbol(
                    candidates
                        .choose(near_uniform, GlyphMode::Hybrid, true, 0.01)
                        .0
                )
            )
        );
    }
    #[test]
    fn complexity_rejects_a_weak_letter_match_but_allows_a_clear_one() {
        let pattern = [0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6];
        let mut c = Candidates {
            glyphs: vec![
                Candidate::new(b'#', [0.5; 8]),
                Candidate::new(b'X', pattern),
            ],
            density_order: vec![0, 1],
            tonal_order: vec![0],
        };
        assert_eq!(
            c.symbol(c.choose(pattern, GlyphMode::Shape, true, 0.0).0),
            b'X'
        );
        assert_eq!(
            c.symbol(c.choose(pattern, GlyphMode::Shape, true, 0.02).0),
            b'#'
        );
        let strong = [0., 1., 0., 1., 0., 1., 0., 1.];
        c.glyphs[1] = Candidate::new(b'X', strong);
        assert_eq!(
            c.symbol(c.choose(strong, GlyphMode::Shape, true, 0.02).0),
            b'X'
        );
    }
    #[test]
    fn stability_does_not_retain_a_structural_glyph_on_a_smooth_surface() {
        let c = Candidates::new(None, GlyphPalette::Structural);
        let diagonal = SHAPE_GLYPHS[0].mask;
        let previous = c.choose(diagonal, GlyphMode::Hybrid, true, 0.01).0;
        let current = c.choose(diagonal, GlyphMode::Hybrid, false, 0.01);
        assert_eq!(
            c.retain(
                previous,
                current,
                diagonal,
                false,
                AsciiOptions {
                    stability: 1.0,
                    ..Default::default()
                }
            ),
            current.0
        );
        let flat = [0.5; 8];
        let current = c.choose(flat, GlyphMode::Hybrid, true, 0.01);
        assert_eq!(
            c.retain(
                previous,
                current,
                flat,
                true,
                AsciiOptions {
                    stability: 1.0,
                    ..Default::default()
                }
            ),
            current.0
        );
    }
}
