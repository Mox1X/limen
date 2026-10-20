use crate::render::Framebuffer;

pub const RAMP: &[u8] = b" .:-=+*#%@";

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
    assert!(samples_x > 0 && samples_y > 0);
    assert_eq!(frame.width(), grid.width() * samples_x);
    assert_eq!(frame.height(), grid.height() * samples_y);
    let divisor = (samples_x * samples_y) as f32;
    for y in 0..grid.height() {
        for x in 0..grid.width() {
            let mut sum = 0.0;
            for sy in 0..samples_y {
                for sx in 0..samples_x {
                    sum += frame.brightness()
                        [(y * samples_y + sy) * frame.width() + x * samples_x + sx];
                }
            }
            grid.set(x, y, brightness_char(sum / divisor));
        }
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
