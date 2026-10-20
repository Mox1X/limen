use glam::{Mat4, Quat, Vec3};

#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<[usize; 3]>,
}

#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}
impl Transform {
    pub fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

pub struct Instance {
    pub mesh: Mesh,
    pub transform: Transform,
}

pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: Vec3::new(0.0, 0.0, 3.5),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov_y: 45.0_f32.to_radians(),
            near: 0.1,
            far: 100.0,
        }
    }
}
impl Camera {
    /// Right-handed, with normalized depth in [0, 1].
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far)
            * Mat4::look_at_rh(self.eye, self.target, self.up)
    }
}

pub struct DirectionalLight {
    /// Direction from the surface toward the light, in world space.
    pub toward_light: Vec3,
    pub ambient: f32,
    pub diffuse: f32,
}
impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            toward_light: Vec3::new(-0.5, 0.8, 1.0).normalize(),
            ambient: 0.12,
            diffuse: 0.88,
        }
    }
}
impl DirectionalLight {
    pub fn intensity(&self, normal: Vec3) -> f32 {
        (self.ambient
            + self.diffuse
                * normal
                    .normalize_or_zero()
                    .dot(self.toward_light.normalize_or_zero())
                    .max(0.0))
        .clamp(0.0, 1.0)
    }
}

pub struct Scene {
    pub instances: Vec<Instance>,
    pub light: DirectionalLight,
}
impl Scene {
    pub fn from_mesh(mesh: Mesh) -> Self {
        Self {
            instances: vec![Instance {
                mesh,
                transform: Transform::default(),
            }],
            light: DirectionalLight::default(),
        }
    }
}

pub fn cube() -> Mesh {
    let mut mesh = Mesh {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    // Separate vertices preserve each face's hard normal.
    for (normal, u, v) in [
        (Vec3::Z, Vec3::X, Vec3::Y),
        (-Vec3::Z, -Vec3::X, Vec3::Y),
        (Vec3::X, -Vec3::Z, Vec3::Y),
        (-Vec3::X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, -Vec3::Z),
        (-Vec3::Y, Vec3::X, Vec3::Z),
    ] {
        let start = mesh.vertices.len();
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            mesh.vertices.push(Vertex {
                position: (normal + a * u + b * v) / 3.0_f32.sqrt(),
                normal,
            });
        }
        mesh.indices
            .extend([[start, start + 1, start + 2], [start, start + 2, start + 3]]);
    }
    mesh
}
