use crate::geometry::{Camera, DirectionalLight, Scene};
use crate::view::ViewMode;
use glam::{Mat3, Vec2, Vec3, Vec4};

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    pub mode: ViewMode,
}

pub struct Framebuffer {
    width: usize,
    height: usize,
    depth: Vec<f32>,
    brightness: Vec<f32>,
}
impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            depth: vec![f32::INFINITY; width * height],
            brightness: vec![0.0; width * height],
        }
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn brightness(&self) -> &[f32] {
        &self.brightness
    }
    pub fn depth(&self) -> &[f32] {
        &self.depth
    }
    fn visualize_depth(&mut self, camera: &Camera) {
        let linear =
            |z: f32| camera.near * camera.far / (camera.far - z * (camera.far - camera.near));
        let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
        for &z in &self.depth {
            if z.is_finite() {
                let distance = linear(z);
                min = min.min(distance);
                max = max.max(distance);
            }
        }
        for (value, &z) in self.brightness.iter_mut().zip(&self.depth) {
            *value = if !z.is_finite() {
                0.0
            } else if max - min <= 1e-6 * max.abs().max(1.0) {
                0.5
            } else {
                0.1 + 0.9 * (1.0 - (linear(z) - min) / (max - min)).clamp(0.0, 1.0)
            };
        }
    }
    pub fn clear(&mut self) {
        self.depth.fill(f32::INFINITY);
        self.brightness.fill(0.0);
    }
    pub fn write(&mut self, x: usize, y: usize, depth: f32, brightness: f32) {
        if x >= self.width
            || y >= self.height
            || !depth.is_finite()
            || !(0.0..=1.0).contains(&depth)
        {
            return;
        }
        let index = y * self.width + x;
        if depth < self.depth[index] {
            self.depth[index] = depth;
            self.brightness[index] = brightness.clamp(0.0, 1.0);
        }
    }
}

#[derive(Clone, Copy)]
struct ClipVertex {
    position: Vec4,
    normal: Vec3,
}
impl ClipVertex {
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            position: self.position.lerp(other.position, t),
            normal: self.normal.lerp(other.normal, t),
        }
    }
}

// Homogeneous Sutherland-Hodgman clipping; glam's RH projection uses 0 <= z <= w.
fn plane_distance(p: Vec4, plane: usize) -> f32 {
    match plane {
        0 => p.w + p.x,
        1 => p.w - p.x,
        2 => p.w + p.y,
        3 => p.w - p.y,
        4 => p.z,
        _ => p.w - p.z,
    }
}
fn clip_triangle(triangle: [ClipVertex; 3]) -> Vec<ClipVertex> {
    let mut polygon = triangle.to_vec();
    let mut output = Vec::with_capacity(9);
    for plane in 0..6 {
        output.clear();
        if polygon.is_empty() {
            break;
        }
        let mut previous = *polygon.last().unwrap();
        let mut previous_d = plane_distance(previous.position, plane);
        for &current in &polygon {
            let current_d = plane_distance(current.position, plane);
            if (previous_d >= 0.0) != (current_d >= 0.0) {
                let t = previous_d / (previous_d - current_d);
                output.push(previous.lerp(current, t));
            }
            if current_d >= 0.0 {
                output.push(current);
            }
            previous = current;
            previous_d = current_d;
        }
        std::mem::swap(&mut polygon, &mut output);
    }
    polygon
}

#[derive(Clone, Copy)]
struct ScreenVertex {
    position: Vec2,
    depth: f32,
    inverse_w: f32,
    normal_over_w: Vec3,
}

fn edge(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}
fn top_left(a: Vec2, b: Vec2) -> bool {
    let delta = b - a;
    delta.y < 0.0 || (delta.y == 0.0 && delta.x > 0.0)
}
fn covered(value: f32, inclusive: bool) -> bool {
    value > 0.0 || (value == 0.0 && inclusive)
}

fn rasterize(
    mut v: [ScreenVertex; 3],
    light: &DirectionalLight,
    frame: &mut Framebuffer,
    mode: ViewMode,
) {
    let mut area = edge(v[0].position, v[1].position, v[2].position);
    if !area.is_finite() || area.abs() < 1e-8 || frame.width == 0 || frame.height == 0 {
        return;
    }
    // Support either winding: OBJ files may contain open, double-sided surfaces.
    if area < 0.0 {
        v.swap(1, 2);
        area = -area;
    }
    let min = v
        .iter()
        .fold(Vec2::splat(f32::INFINITY), |a, v| a.min(v.position));
    let max = v
        .iter()
        .fold(Vec2::splat(f32::NEG_INFINITY), |a, v| a.max(v.position));
    let x0 = (min.x - 0.5).ceil().max(0.0) as usize;
    let y0 = (min.y - 0.5).ceil().max(0.0) as usize;
    let x1 = (max.x - 0.5).floor().min(frame.width as f32 - 1.0) as usize;
    let y1 = (max.y - 0.5).floor().min(frame.height as f32 - 1.0) as usize;
    let inclusions = [
        top_left(v[1].position, v[2].position),
        top_left(v[2].position, v[0].position),
        top_left(v[0].position, v[1].position),
    ];
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let edges = [
                edge(v[1].position, v[2].position, p),
                edge(v[2].position, v[0].position, p),
                edge(v[0].position, v[1].position, p),
            ];
            if !(0..3).all(|i| covered(edges[i], inclusions[i])) {
                continue;
            }
            let b = edges.map(|e| e / area);
            // Difference form keeps a constant-depth plane exactly constant.
            let depth =
                v[0].depth + b[1] * (v[1].depth - v[0].depth) + b[2] * (v[2].depth - v[0].depth);
            if depth >= frame.depth[y * frame.width + x] {
                continue;
            }
            let inverse_w = b[0] * v[0].inverse_w + b[1] * v[1].inverse_w + b[2] * v[2].inverse_w;
            if inverse_w <= 0.0 {
                continue;
            }
            let normal =
                (b[0] * v[0].normal_over_w + b[1] * v[1].normal_over_w + b[2] * v[2].normal_over_w)
                    / inverse_w;
            let brightness = match mode {
                ViewMode::Shaded => light.intensity(normal),
                ViewMode::Silhouette | ViewMode::Depth => 1.0,
                _ => normal.normalize_or_zero()[mode.normal_axis().unwrap()] * 0.5 + 0.5,
            };
            frame.write(x, y, depth, brightness);
        }
    }
}

/// `aspect` is the physical display aspect (columns * cell width / rows / cell height),
/// independent of the internal sample grid's aspect.
pub fn render(scene: &Scene, camera: &Camera, aspect: f32, frame: &mut Framebuffer) {
    render_with_options(scene, camera, aspect, frame, RenderOptions::default());
}

pub fn render_with_options(
    scene: &Scene,
    camera: &Camera,
    aspect: f32,
    frame: &mut Framebuffer,
    options: RenderOptions,
) {
    frame.clear();
    if frame.width == 0 || frame.height == 0 || !aspect.is_finite() || aspect <= 0.0 {
        return;
    }
    let view_projection = camera.view_projection(aspect);
    for instance in &scene.instances {
        let model = instance.transform.matrix();
        if !model.is_finite() || model.determinant().abs() < 1e-12 {
            continue;
        }
        let mut normals = Mat3::from_mat4(model).inverse().transpose();
        if options.mode.normal_axis().is_some() {
            normals = Mat3::from_mat4(camera.view_matrix()) * normals;
        }
        let mvp = view_projection * model;
        let vertices: Vec<_> = instance
            .mesh
            .vertices
            .iter()
            .map(|v| ClipVertex {
                position: mvp * v.position.extend(1.0),
                normal: normals * v.normal,
            })
            .collect();
        for indices in &instance.mesh.indices {
            let triangle = indices.map(|i| vertices[i]);
            if triangle
                .iter()
                .any(|v| !v.position.is_finite() || !v.normal.is_finite())
            {
                continue;
            }
            let polygon = clip_triangle(triangle);
            if polygon.len() < 3 {
                continue;
            }
            for i in 1..polygon.len() - 1 {
                let clipped = [polygon[0], polygon[i], polygon[i + 1]];
                if clipped.iter().any(|v| v.position.w <= 0.0) {
                    continue;
                }
                let screen = clipped.map(|v| {
                    let inverse_w = 1.0 / v.position.w;
                    let ndc = v.position.truncate() * inverse_w;
                    ScreenVertex {
                        position: Vec2::new(
                            (ndc.x * 0.5 + 0.5) * frame.width as f32,
                            (0.5 - ndc.y * 0.5) * frame.height as f32,
                        ),
                        depth: ndc.z,
                        inverse_w,
                        normal_over_w: v.normal * inverse_w,
                    }
                });
                rasterize(screen, &scene.light, frame, options.mode);
            }
        }
    }
    if options.mode == ViewMode::Depth {
        frame.visualize_depth(camera);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vertex(x: f32, y: f32, depth: f32, normal: Vec3) -> ScreenVertex {
        ScreenVertex {
            position: Vec2::new(x, y),
            depth,
            inverse_w: 1.0,
            normal_over_w: normal,
        }
    }
    #[test]
    fn shared_edge_has_no_cracks_or_double_coverage() {
        let light = DirectionalLight::default();
        let mut a = Framebuffer::new(4, 4);
        let mut b = Framebuffer::new(4, 4);
        let vertices =
            [(0., 0.), (4., 0.), (4., 4.), (0., 4.)].map(|(x, y)| vertex(x, y, 0.5, Vec3::Z));
        rasterize(
            [vertices[0], vertices[1], vertices[2]],
            &light,
            &mut a,
            ViewMode::Shaded,
        );
        rasterize(
            [vertices[0], vertices[2], vertices[3]],
            &light,
            &mut b,
            ViewMode::Shaded,
        );
        for i in 0..16 {
            assert_ne!(a.depth[i].is_finite(), b.depth[i].is_finite());
        }
    }
    #[test]
    fn depth_is_order_independent() {
        let light = DirectionalLight::default();
        let near = [
            vertex(0., 0., 0.2, Vec3::Z),
            vertex(4., 0., 0.2, Vec3::Z),
            vertex(0., 4., 0.2, Vec3::Z),
        ];
        let far = near.map(|mut v| {
            v.depth = 0.8;
            v.normal_over_w = -Vec3::Z;
            v
        });
        let mut a = Framebuffer::new(4, 4);
        let mut b = Framebuffer::new(4, 4);
        for t in [near, far] {
            rasterize(t, &light, &mut a, ViewMode::Shaded);
        }
        for t in [far, near] {
            rasterize(t, &light, &mut b, ViewMode::Shaded);
        }
        assert_eq!(a.depth, b.depth);
        assert_eq!(a.brightness, b.brightness);
    }
    #[test]
    fn degenerate_triangle_is_ignored() {
        let mut frame = Framebuffer::new(4, 4);
        rasterize(
            [vertex(1., 1., 0.5, Vec3::Z); 3],
            &DirectionalLight::default(),
            &mut frame,
            ViewMode::Shaded,
        );
        assert!(frame.depth.iter().all(|z| z.is_infinite()));
    }
    #[test]
    fn clipping_respects_all_six_planes() {
        for position in [
            Vec4::new(-2., 0., 0.5, 1.),
            Vec4::new(2., 0., 0.5, 1.),
            Vec4::new(0., -2., 0.5, 1.),
            Vec4::new(0., 2., 0.5, 1.),
            Vec4::new(0., 0.5, -1., 1.),
            Vec4::new(0., 0.5, 2., 1.),
        ] {
            let v = |position| ClipVertex {
                position,
                normal: Vec3::Z,
            };
            let polygon = clip_triangle([
                v(position),
                v(Vec4::new(-0.3, -0.3, 0.5, 1.)),
                v(Vec4::new(0.3, -0.3, 0.5, 1.)),
            ]);
            assert!(polygon.len() >= 3);
            for p in polygon {
                for plane in 0..6 {
                    assert!(plane_distance(p.position, plane) >= -1e-6);
                }
            }
        }
        let v = ClipVertex {
            position: Vec4::new(0., 0., -1., 1.),
            normal: Vec3::Z,
        };
        assert!(clip_triangle([v; 3]).is_empty());
    }
    #[test]
    fn smooth_normals_use_perspective_weights() {
        let light = DirectionalLight {
            toward_light: Vec3::Z,
            ambient: 0.0,
            diffuse: 1.0,
        };
        let mut v = [
            vertex(0., 0., 0.5, Vec3::X),
            vertex(4., 0., 0.5, Vec3::Z),
            vertex(0., 4., 0.5, Vec3::Z),
        ];
        v[0].inverse_w = 0.25;
        v[0].normal_over_w *= 0.25;
        let mut frame = Framebuffer::new(4, 4);
        rasterize(v, &light, &mut frame, ViewMode::Shaded);
        // At (0.5, 0.5), barycentric weights are .75, .125, .125.
        let expected = Vec3::new(0.75 * 0.25, 0., 0.25).normalize().z;
        assert!((frame.brightness[0] - expected).abs() < 1e-6);
        assert!(frame.brightness[2] > frame.brightness[0]);
        assert!(light.intensity(Vec3::Z) > light.intensity(-Vec3::Z));
    }
    #[test]
    fn cube_is_visible() {
        let mut frame = Framebuffer::new(240, 160);
        render(
            &Scene::from_mesh(crate::geometry::cube()),
            &Camera::default(),
            1.5,
            &mut frame,
        );
        assert!(frame.brightness.iter().filter(|&&b| b > 0.0).count() > 1000);
        assert!(
            frame
                .depth
                .iter()
                .all(|z| z.is_infinite() || (0.0..=1.0).contains(z))
        );
    }
}
