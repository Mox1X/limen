//! Headless release benchmark: cargo run --release --example benchmark
use glam::Quat;
use limen::{
    ascii::{CharGrid, resolve},
    geometry::{Camera, Scene, cube},
    obj,
    render::{Framebuffer, render},
};
use std::{hint::black_box, path::Path, time::Instant};
fn main() {
    let obj = obj::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/smooth.obj"))
        .unwrap();
    for (name, mesh) in [("cube", cube()), ("smooth.obj", obj)] {
        let triangles = mesh.indices.len();
        let mut scene = Scene::from_mesh(mesh);
        let camera = Camera::default();
        let mut frame = Framebuffer::new(240, 160);
        let mut grid = CharGrid::new(120, 40);
        let start = Instant::now();
        for i in 0..300 {
            scene.instances[0].transform.rotation = Quat::from_rotation_y(i as f32 * 0.01);
            render(&scene, &camera, 1.5, &mut frame);
            resolve(&frame, 2, 4, &mut grid);
            black_box(&grid);
        }
        let milliseconds = start.elapsed().as_secs_f64() * 1000.0 / 300.0;
        println!(
            "{name}: {triangles} triangles, 120x40 chars, 2x4 samples, {milliseconds:.3} ms/frame ({:.0} FPS), excluding terminal I/O",
            1000.0 / milliseconds
        );
    }
}
