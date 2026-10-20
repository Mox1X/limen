use crate::geometry::{Mesh, Vertex};
use glam::Vec3;
use std::{error::Error, path::Path};

type LoadResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug)]
pub struct ObjectInfo {
    pub name: String,
    pub triangles: usize,
}

fn read_models(path: &Path) -> LoadResult<Vec<tobj::Model>> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let (models, _) = tobj::load_obj_buf(
        &mut reader,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ignore_points: true,
            ignore_lines: true,
        },
        |_| Ok((Vec::new(), Default::default())),
    )?;
    Ok(models)
}

/// Lists OBJ object/group names before normalization or degenerate-face removal.
pub fn list_objects(path: &Path) -> LoadResult<Vec<ObjectInfo>> {
    Ok(read_models(path)?
        .into_iter()
        .map(|model| ObjectInfo {
            name: model.name,
            triangles: model.mesh.indices.len() / 3,
        })
        .collect())
}

/// Loads geometry only: material files and texture coordinates are not required.
/// A unified index preserves OBJ normal discontinuities at hard edges.
pub fn load(path: &Path) -> LoadResult<Mesh> {
    load_selected(path, &[])
}

/// Selects named objects before computing bounds, preserving their relative positions.
/// An empty selection loads all objects. Repeated names select every matching part.
pub fn load_selected(path: &Path, selected: &[String]) -> LoadResult<Mesh> {
    let models = read_models(path)?;
    for name in selected {
        if !models.iter().any(|model| &model.name == name) {
            let available = models
                .iter()
                .map(|model| model.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(
                format!("OBJ object '{name}' not found; available objects: {available}").into(),
            );
        }
    }
    let mut mesh = Mesh {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    for model in models {
        if !selected.is_empty() && !selected.contains(&model.name) {
            continue;
        }
        let source = model.mesh;
        let has_normals = source.normals.len() == source.positions.len();
        for triangle in source.indices.as_chunks::<3>().0 {
            let mut positions = [Vec3::ZERO; 3];
            let mut normals = [Vec3::ZERO; 3];
            for corner in 0..3 {
                let offset = triangle[corner] as usize * 3;
                let p = source
                    .positions
                    .get(offset..offset + 3)
                    .ok_or("OBJ contains an invalid vertex index")?;
                positions[corner] = Vec3::from_slice(p);
                if !positions[corner].is_finite() {
                    return Err("OBJ contains non-finite coordinates".into());
                }
                if has_normals {
                    normals[corner] = Vec3::from_slice(&source.normals[offset..offset + 3]);
                }
            }
            let cross = (positions[1] - positions[0]).cross(positions[2] - positions[0]);
            if cross.length_squared() <= 1e-20 {
                continue;
            }
            let flat = cross.normalize();
            let start = mesh.vertices.len();
            for corner in 0..3 {
                let normal = normals[corner];
                mesh.vertices.push(Vertex {
                    position: positions[corner],
                    normal: if normal.is_finite() && normal.length_squared() > 1e-20 {
                        normal.normalize()
                    } else {
                        flat
                    },
                });
            }
            mesh.indices.push([start, start + 1, start + 2]);
        }
    }
    if mesh.indices.is_empty() {
        return Err("OBJ contains no non-degenerate triangles".into());
    }
    let min = mesh
        .vertices
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |a, v| a.min(v.position));
    let max = mesh
        .vertices
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |a, v| a.max(v.position));
    let center = min * 0.5 + max * 0.5;
    let radius = mesh
        .vertices
        .iter()
        .map(|v| (v.position - center).length())
        .fold(0.0_f32, f32::max);
    if !radius.is_finite() || radius <= 0.0 {
        return Err("OBJ bounds cannot be normalized".into());
    }
    for vertex in &mut mesh.vertices {
        vertex.position = (vertex.position - center) / radius;
    }
    Ok(mesh)
}
