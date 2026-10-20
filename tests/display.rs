use glam::{Quat, Vec3};
use limen::{
    ascii::{CharGrid, ToneMap, brightness_char, resolve_with_tone},
    geometry::{BoundingSphere, Camera, Instance, Mesh, Scene, Transform, Vertex, cube},
    render::{Framebuffer, RenderOptions, render_with_options},
    view::{ViewMode, ViewSettings},
};

fn triangle() -> Mesh {
    Mesh {
        vertices: [
            Vec3::new(-0.7, -0.7, 0.0),
            Vec3::new(0.7, -0.7, 0.0),
            Vec3::new(0.0, 0.7, 0.0),
        ]
        .map(|position| Vertex {
            position,
            normal: Vec3::Z,
        })
        .to_vec(),
        indices: vec![[0, 1, 2]],
    }
}
fn draw(scene: &Scene, camera: &Camera, mode: ViewMode) -> Framebuffer {
    let mut frame = Framebuffer::new(120, 80);
    render_with_options(
        scene,
        camera,
        1.5,
        &mut frame,
        RenderOptions {
            mode,
            ..Default::default()
        },
    );
    frame
}
fn visible(frame: &Framebuffer) -> usize {
    frame.depth().iter().filter(|z| z.is_finite()).count()
}

#[test]
fn tone_mapping_is_monotone_and_preserves_uncovered_background() {
    for contrast in [0.25, 1.0, 1.2, 3.0] {
        for gamma in [0.25, 1.0, 4.0] {
            let tone = ToneMap { contrast, gamma };
            let values: Vec<_> = (0..=100).map(|i| tone.apply(i as f32 / 100.0)).collect();
            assert!(values.windows(2).all(|p| p[0] <= p[1]));
            assert!(values.iter().all(|v| (0.0..=1.0).contains(v)));
            let mut frame = Framebuffer::new(4, 4);
            frame.write(0, 0, 0.5, 1.0);
            let mut grid = CharGrid::new(2, 1);
            resolve_with_tone(&frame, 2, 4, &mut grid, tone);
            assert_eq!(grid.get(0, 0), Some(brightness_char(tone.apply(0.125))));
            assert_eq!(grid.get(1, 0), Some(b' '));
        }
    }
    assert_eq!(ToneMap::LINEAR.apply(0.25), 0.25);
    assert_eq!(
        ToneMap {
            contrast: 1.0,
            gamma: 2.0
        }
        .apply(0.5),
        0.25
    );
}

#[test]
fn fitting_keeps_the_sphere_inside_portrait_and_landscape_views() {
    let bounds = BoundingSphere {
        center: Vec3::new(3.0, 2.0, -1.0),
        radius: 2.0,
    };
    for aspect in [0.2, 0.5, 1.5, 3.0] {
        let camera = Camera::fitted(bounds, aspect, 1.0);
        let projection = camera.view_projection(aspect);
        for y in 0..=40 {
            for x in 0..80 {
                let latitude = std::f32::consts::PI * y as f32 / 40.0;
                let longitude = std::f32::consts::TAU * x as f32 / 80.0;
                let point = bounds.center
                    + bounds.radius
                        * Vec3::new(
                            latitude.sin() * longitude.cos(),
                            latitude.cos(),
                            latitude.sin() * longitude.sin(),
                        );
                let clip = projection * point.extend(1.0);
                let ndc = clip.truncate() / clip.w;
                assert!(ndc.x.abs() <= 0.901 && ndc.y.abs() <= 0.901);
                assert!((0.0..=1.0).contains(&ndc.z));
            }
        }
    }
}

#[test]
fn sampling_does_not_change_physical_proportions() {
    // A sphere-like circular disc makes both projected radii easy to measure.
    let segments = 96;
    let mut vertices = vec![Vertex {
        position: Vec3::ZERO,
        normal: Vec3::Z,
    }];
    for i in 0..segments {
        let a = std::f32::consts::TAU * i as f32 / segments as f32;
        vertices.push(Vertex {
            position: Vec3::new(a.cos(), a.sin(), 0.0),
            normal: Vec3::Z,
        });
    }
    let indices = (0..segments)
        .map(|i| [0, i + 1, (i + 1) % segments + 1])
        .collect();
    let scene = Scene::from_mesh(Mesh { vertices, indices });
    for (cols, rows, cell) in [(120, 40, 0.5), (80, 50, 0.7), (60, 60, 0.4)] {
        let aspect = cols as f32 * cell / rows as f32;
        let camera = Camera::fitted(
            BoundingSphere {
                center: Vec3::ZERO,
                radius: 1.0,
            },
            aspect,
            1.0,
        );
        for (sx, sy) in [(1, 1), (2, 4), (4, 2)] {
            let mut frame = Framebuffer::new(cols * sx, rows * sy);
            render_with_options(
                &scene,
                &camera,
                aspect,
                &mut frame,
                RenderOptions {
                    mode: ViewMode::Silhouette,
                    ..Default::default()
                },
            );
            let points: Vec<_> = frame
                .depth()
                .iter()
                .enumerate()
                .filter(|(_, z)| z.is_finite())
                .map(|(i, _)| (i % frame.width(), i / frame.width()))
                .collect();
            let w = points.iter().map(|p| p.0).max().unwrap()
                - points.iter().map(|p| p.0).min().unwrap()
                + 1;
            let h = points.iter().map(|p| p.1).max().unwrap()
                - points.iter().map(|p| p.1).min().unwrap()
                + 1;
            let physical_ratio = (w as f32 / sx as f32 * cell) / (h as f32 / sy as f32);
            assert!(
                (physical_ratio - 1.0).abs() < 0.06,
                "{cols}x{rows}, {sx}x{sy}: {physical_ratio}"
            );
        }
    }
}

#[test]
fn zoom_enlarges_the_model_without_changing_orientation() {
    let mut scene = Scene::from_mesh(cube());
    scene.instances[0].transform.rotation = Quat::from_rotation_y(0.4);
    let bounds = scene.instances[0].mesh.bounding_sphere().unwrap();
    let a = draw(
        &scene,
        &Camera::fitted(bounds, 1.5, 1.0),
        ViewMode::Silhouette,
    );
    let b = draw(
        &scene,
        &Camera::fitted(bounds, 1.5, 1.2),
        ViewMode::Silhouette,
    );
    assert!(visible(&b) > visible(&a));
    assert_eq!(
        scene.instances[0].transform.rotation,
        Quat::from_rotation_y(0.4)
    );
}

#[test]
fn debug_modes_preserve_visibility_and_expose_camera_space_normals() {
    let mut scene = Scene::from_mesh(triangle());
    scene.instances[0].transform.translation = Vec3::new(0.1, 0.0, -0.5);
    let camera = Camera::default();
    let reference = draw(&scene, &camera, ViewMode::Shaded);
    for mode in ViewMode::ALL {
        let frame = draw(&scene, &camera, mode);
        assert_eq!(reference.depth(), frame.depth());
        for (i, &depth) in frame.depth().iter().enumerate() {
            if !depth.is_finite() {
                assert_eq!(frame.brightness()[i], 0.0);
                continue;
            }
            let expected = match mode {
                ViewMode::Depth | ViewMode::NormalX | ViewMode::NormalY => 0.5,
                ViewMode::Silhouette | ViewMode::NormalZ => 1.0,
                _ => continue,
            };
            assert!((frame.brightness()[i] - expected).abs() < 1e-5);
        }
    }
    // Changing the viewing direction changes the normal components, not the world light.
    let mut side_camera = camera.clone();
    side_camera.eye = Vec3::new(2.0, 0.0, 3.5);
    let frame = draw(&scene, &side_camera, ViewMode::NormalX);
    let expected = (side_camera.view_matrix().transform_vector3(Vec3::Z).x + 1.0) * 0.5;
    assert!(
        frame
            .depth()
            .iter()
            .zip(frame.brightness())
            .filter(|(z, _)| z.is_finite())
            .all(|(_, b)| (b - expected).abs() < 1e-5)
    );
}

#[test]
fn depth_is_linearized_and_nearer_surfaces_are_brighter() {
    let scene = Scene {
        instances: vec![
            Instance {
                mesh: triangle(),
                transform: Transform {
                    translation: Vec3::new(-0.8, 0.0, 0.0),
                    ..Default::default()
                },
            },
            Instance {
                mesh: triangle(),
                transform: Transform {
                    translation: Vec3::new(0.8, 0.0, -1.0),
                    ..Default::default()
                },
            },
        ],
        light: ViewSettings::default().light(),
    };
    let frame = draw(&scene, &Camera::default(), ViewMode::Depth);
    let mut samples: Vec<_> = frame
        .depth()
        .iter()
        .zip(frame.brightness())
        .filter(|(z, _)| z.is_finite())
        .collect();
    samples.sort_by(|a, b| a.0.total_cmp(b.0));
    assert!((samples.first().unwrap().1 - 1.0).abs() < 1e-5);
    assert!((samples.last().unwrap().1 - 0.1).abs() < 1e-5);
    assert!(samples.windows(2).all(|p| p[0].1 + 1e-5 >= *p[1].1));
    let empty = draw(
        &Scene {
            instances: vec![],
            light: ViewSettings::default().light(),
        },
        &Camera::default(),
        ViewMode::Depth,
    );
    assert!(empty.brightness().iter().all(|&b| b == 0.0));
}

#[test]
fn model_rotation_does_not_rotate_the_light() {
    let settings = ViewSettings::default();
    let mut scene = Scene::from_mesh(cube());
    scene.light = settings.light();
    let original = scene.light.toward_light;
    let before = draw(&scene, &Camera::default(), ViewMode::Shaded);
    scene.instances[0].transform.rotation = Quat::from_rotation_y(0.5);
    let after = draw(&scene, &Camera::default(), ViewMode::Shaded);
    assert_eq!(scene.light.toward_light, original);
    assert_ne!(before.brightness(), after.brightness());
}
