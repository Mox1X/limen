//! Shared area resampling for scene cells and font bitmaps.
pub(crate) enum CellSampler {
    Direct,
    Area([Vec<(usize, usize, f32)>; 8]),
}
impl CellSampler {
    pub(crate) fn new(width: usize, height: usize) -> Self {
        if (width, height) == (2, 4) {
            return Self::Direct;
        }
        let weights = std::array::from_fn(|i| {
            let left = (i % 2) as f32 * width as f32 / 2.0;
            let right = (i % 2 + 1) as f32 * width as f32 / 2.0;
            let top = (i / 2) as f32 * height as f32 / 4.0;
            let bottom = (i / 2 + 1) as f32 * height as f32 / 4.0;
            let area = (right - left) * (bottom - top);
            let mut entries = Vec::new();
            for y in top.floor() as usize..bottom.ceil() as usize {
                for x in left.floor() as usize..right.ceil() as usize {
                    let overlap_x = (right.min(x as f32 + 1.0) - left.max(x as f32)).max(0.0);
                    let overlap_y = (bottom.min(y as f32 + 1.0) - top.max(y as f32)).max(0.0);
                    let weight = overlap_x * overlap_y / area;
                    if weight > 0.0 {
                        entries.push((x, y, weight));
                    }
                }
            }
            entries
        });
        Self::Area(weights)
    }
    pub(crate) fn sample(
        &self,
        mut read: impl FnMut(usize, usize) -> (f32, bool),
    ) -> ([f32; 8], [bool; 8]) {
        let mut values = [0.0; 8];
        let mut covered = [false; 8];
        for i in 0..8 {
            match self {
                Self::Direct => {
                    (values[i], covered[i]) = read(i % 2, i / 2);
                }
                Self::Area(weights) => {
                    for &(x, y, weight) in &weights[i] {
                        let (value, visible) = read(x, y);
                        values[i] += value * weight;
                        covered[i] |= visible;
                    }
                }
            }
        }
        (values, covered)
    }
}
