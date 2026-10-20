use glam::Vec3;
use limen::{
    geometry::{Camera, Scene},
    obj,
    render::{self, Framebuffer},
};
use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
#[test]
fn all_objects_and_supplied_normals_are_preserved() {
    let mesh = obj::load(&fixture("smooth.obj")).unwrap();
    assert_eq!(mesh.indices.len(), 8);
    for v in &mesh.vertices {
        assert!((v.position.length() - 1.0).abs() < 1e-6);
        assert!((v.normal - v.position).length() < 1e-6);
    }
    let mut frame = Framebuffer::new(240, 160);
    render::render(&Scene::from_mesh(mesh), &Camera::default(), 1.5, &mut frame);
    assert!(frame.brightness().iter().filter(|&&b| b > 0.0).count() > 1000);
}
#[test]
fn quads_negative_indices_and_flat_fallback() {
    let mesh = obj::load(&fixture("flat.obj")).unwrap();
    assert_eq!(mesh.indices.len(), 2);
    let center =
        mesh.vertices.iter().fold(Vec3::ZERO, |a, v| a + v.position) / mesh.vertices.len() as f32;
    // Triangle expansion duplicates the diagonal symmetrically for this quad.
    assert!(center.length() < 1e-6);
    assert!(
        mesh.vertices
            .iter()
            .all(|v| v.normal == Vec3::Z && v.position.length() <= 1.000001)
    );
}
#[test]
fn invalid_and_missing_files_are_errors() {
    assert!(obj::load(&fixture("invalid.obj")).is_err());
    assert!(obj::load(&fixture("missing.obj")).is_err());
}

#[test]
fn named_selection_removes_enclosure_before_normalization() {
    let path = fixture("enclosed.obj");
    let objects = obj::list_objects(&path).unwrap();
    assert_eq!(
        objects
            .iter()
            .map(|o| (o.name.as_str(), o.triangles))
            .collect::<Vec<_>>(),
        vec![("subject", 1), ("enclosure", 12)]
    );
    let all = obj::load(&path).unwrap();
    let selected = obj::load_selected(&path, &["subject".into()]).unwrap();
    assert_eq!(all.indices.len(), 13);
    assert_eq!(selected.indices.len(), 1);
    let radius = selected
        .vertices
        .iter()
        .map(|v| v.position.length())
        .fold(0.0_f32, f32::max);
    assert!((radius - 1.0).abs() < 1e-6);
    let combined = obj::load_selected(&path, &["subject".into(), "enclosure".into()]).unwrap();
    assert_eq!(combined.indices.len(), all.indices.len());
    let error = obj::load_selected(&path, &["missing".into()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("subject") && error.contains("enclosure") && error.contains("missing"));
    let mut frame = Framebuffer::new(240, 160);
    render::render(&Scene::from_mesh(all), &Camera::default(), 1.5, &mut frame);
    let enclosed = frame.brightness().to_vec();
    render::render(
        &Scene::from_mesh(selected),
        &Camera::default(),
        1.5,
        &mut frame,
    );
    assert_ne!(enclosed, frame.brightness());
}
