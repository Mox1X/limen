//! Display settings shared by the terminal application and headless tools.
use crate::ascii::{GlyphMode, ToneMap};
use crate::geometry::DirectionalLight;
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewMode {
    #[default]
    Shaded,
    Depth,
    Silhouette,
    NormalX,
    NormalY,
    NormalZ,
}
impl ViewMode {
    pub const ALL: [Self; 6] = [
        Self::Shaded,
        Self::Depth,
        Self::Silhouette,
        Self::NormalX,
        Self::NormalY,
        Self::NormalZ,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shaded => "shaded",
            Self::Depth => "depth",
            Self::Silhouette => "silhouette",
            Self::NormalX => "normal-x",
            Self::NormalY => "normal-y",
            Self::NormalZ => "normal-z",
        }
    }
    pub fn normal_axis(self) -> Option<usize> {
        match self {
            Self::NormalX => Some(0),
            Self::NormalY => Some(1),
            Self::NormalZ => Some(2),
            _ => None,
        }
    }
}
impl fmt::Display for ViewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl FromStr for ViewMode {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.as_str() == value)
            .ok_or_else(|| {
                "expected shaded, depth, silhouette, normal-x, normal-y or normal-z".into()
            })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ViewSettings {
    pub mode: ViewMode,
    pub glyph_mode: GlyphMode,
    pub zoom: f32,
    pub light_azimuth: f32,
    pub light_elevation: f32,
    pub ambient: f32,
    pub tone: ToneMap,
}
impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            mode: ViewMode::Shaded,
            glyph_mode: GlyphMode::Brightness,
            zoom: 1.0,
            light_azimuth: -55.0,
            light_elevation: 35.0,
            ambient: 0.06,
            tone: ToneMap::default(),
        }
    }
}
impl ViewSettings {
    pub fn light(self) -> DirectionalLight {
        DirectionalLight::from_angles(self.light_azimuth, self.light_elevation, self.ambient)
    }

    pub fn effective_tone(self) -> ToneMap {
        if self.mode == ViewMode::Shaded {
            self.tone
        } else {
            ToneMap::LINEAR
        }
    }
    /// Terminal-independent actions also make headless experiments reproducible.
    pub fn adjust(&mut self, adjustment: Adjustment) {
        match adjustment {
            Adjustment::Zoom(factor) => self.zoom = (self.zoom * factor).clamp(0.5, 3.0),
            Adjustment::Azimuth(degrees) => {
                self.light_azimuth =
                    (self.light_azimuth + degrees + 180.0).rem_euclid(360.0) - 180.0
            }
            Adjustment::Elevation(degrees) => {
                self.light_elevation = (self.light_elevation + degrees).clamp(-90.0, 90.0)
            }
            Adjustment::Contrast(delta) => {
                self.tone.contrast = (self.tone.contrast + delta).clamp(0.25, 3.0)
            }
            Adjustment::Gamma(delta) => {
                self.tone.gamma = (self.tone.gamma + delta).clamp(0.25, 4.0)
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum Adjustment {
    Zoom(f32),
    Azimuth(f32),
    Elevation(f32),
    Contrast(f32),
    Gamma(f32),
}

#[derive(Debug, PartialEq, Eq)]
pub enum AspectSource {
    Manual,
    Pixels,
    Fallback,
}
impl fmt::Display for AspectSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Manual => "manual",
            Self::Pixels => "auto",
            Self::Fallback => "fallback",
        })
    }
}
pub struct CellAspect {
    pub value: f32,
    pub source: AspectSource,
}
impl CellAspect {
    /// Pixel dimensions are for the full terminal, including any status row.
    pub fn detect(
        manual: Option<f32>,
        columns: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Self {
        if let Some(value) = manual {
            return Self {
                value,
                source: AspectSource::Manual,
            };
        }
        if columns > 0 && rows > 0 && pixel_width >= columns && pixel_height >= rows {
            let value = (pixel_width as f32 / columns as f32) / (pixel_height as f32 / rows as f32);
            if (0.05..=5.0).contains(&value) {
                return Self {
                    value,
                    source: AspectSource::Pixels,
                };
            }
        }
        Self {
            value: 0.5,
            source: AspectSource::Fallback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    #[test]
    fn aspect_detection_precedence_and_resize() {
        let aspect = CellAspect::detect(None, 120, 40, 1200, 800);
        assert_eq!(aspect.source, AspectSource::Pixels);
        assert_eq!(aspect.value, 0.5);
        let resized = CellAspect::detect(None, 80, 24, 800, 600);
        assert_eq!(resized.value, 0.4);
        let manual = CellAspect::detect(Some(0.6), 120, 40, 1200, 800);
        assert_eq!(manual.source, AspectSource::Manual);
        assert_eq!(manual.value, 0.6);
        for dimensions in [
            (0, 0, 0, 0),
            (120, 40, 0, 0),
            (120, 40, 2, 800),
            (1, 1, 65535, 1),
        ] {
            let (cols, rows, w, h) = dimensions;
            assert_eq!(
                CellAspect::detect(None, cols, rows, w, h).source,
                AspectSource::Fallback
            );
        }
    }
    #[test]
    fn light_angles_follow_world_axes() {
        let mut settings = ViewSettings {
            light_azimuth: 0.0,
            light_elevation: 0.0,
            ..Default::default()
        };
        assert!((settings.light().toward_light - Vec3::Z).length() < 1e-6);
        settings.light_azimuth = 90.0;
        assert!((settings.light().toward_light - Vec3::X).length() < 1e-6);
        settings.light_elevation = 90.0;
        assert!((settings.light().toward_light - Vec3::Y).length() < 1e-6);
    }
    #[test]
    fn adjustments_clamp_and_debug_bypasses_tone() {
        let mut settings = ViewSettings::default();
        settings.adjust(Adjustment::Zoom(100.0));
        settings.adjust(Adjustment::Elevation(1000.0));
        settings.adjust(Adjustment::Contrast(-100.0));
        settings.adjust(Adjustment::Gamma(100.0));
        assert_eq!(
            (
                settings.zoom,
                settings.light_elevation,
                settings.tone.contrast,
                settings.tone.gamma
            ),
            (3.0, 90.0, 0.25, 4.0)
        );
        settings.mode = ViewMode::Depth;
        assert_eq!(settings.effective_tone(), ToneMap::LINEAR);
    }
}
