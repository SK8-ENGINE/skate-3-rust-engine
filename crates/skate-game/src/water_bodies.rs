//! Water body sizes for the water shader. Maps split one body of water into
//! several render materials along a 100 m grid (DownTown's Aletown canal is six
//! chunks, some a few metres wide), so a material's own extent says little.
//! Triangles of the water materials that share a vertex position are grouped
//! into bodies; each material gets the surface area of the largest body it
//! belongs to. The shader calms small bodies (fountains) — a project choice,
//! see docs/hails-additions/09-water.md.
use skate_data::skate_map::Vertex;
use std::collections::HashMap;

/// Positions closer than this (metres) count as the same vertex.
const WELD: f32 = 0.05;

/// Area (m²) of the water body each material belongs to, indexed like
/// `materials` (0 for non-water). `water[i]` marks material `i` (vertex
/// `material` is 1-based).
pub(crate) fn body_areas(vertices: &[Vertex], indices: &[u32], water: &[bool]) -> Vec<f32> {
    let material_of = |v: &Vertex| (v.material as usize).checked_sub(1).filter(|&m| water.get(m) == Some(&true));
    let triangles: Vec<[usize; 3]> = indices
        .chunks_exact(3)
        .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
        .filter(|t| t.iter().all(|&i| i < vertices.len()) && material_of(&vertices[t[0]]).is_some())
        .collect();

    let mut parent: Vec<usize> = (0..triangles.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut first = HashMap::<[i32; 3], usize>::new();
    for (t, triangle) in triangles.iter().enumerate() {
        for &i in triangle {
            let key = vertices[i].position.map(|c| (c / WELD).round() as i32);
            let other = *first.entry(key).or_insert(t);
            let (a, b) = (root(&mut parent, t), root(&mut parent, other));
            parent[a] = b;
        }
    }

    let mut area = vec![0.0_f32; triangles.len()];
    for (t, triangle) in triangles.iter().enumerate() {
        let [a, b, c] = triangle.map(|i| vertices[i].position);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let r = root(&mut parent, t);
        area[r] += 0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    }

    let mut out = vec![0.0_f32; water.len()];
    for (t, triangle) in triangles.iter().enumerate() {
        let r = root(&mut parent, t);
        if let Some(m) = material_of(&vertices[triangle[0]]) {
            out[m] = out[m].max(area[r]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(position: [f32; 3], material: u32) -> Vertex {
        Vertex {
            position,
            normal: [0.0, 1.0, 0.0],
            uv: [0.0; 2],
            lightmap_uv: [0.0; 2],
            material,
            decal_uv: None,
            tangent_frame: None,
        }
    }

    /// A `w` x `d` quad at x offset `x0`, own vertices, given material.
    fn quad(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, x0: f32, w: f32, d: f32, material: u32) {
        let base = vertices.len() as u32;
        for p in [[x0, 0.0, 0.0], [x0 + w, 0.0, 0.0], [x0 + w, 0.0, d], [x0, 0.0, d]] {
            vertices.push(vertex(p, material));
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    #[test]
    fn chunks_of_one_body_share_its_whole_area() {
        let (mut v, mut i) = (Vec::new(), Vec::new());
        // One 12x10 body split into two materials at x = 10.
        quad(&mut v, &mut i, 0.0, 10.0, 10.0, 1);
        quad(&mut v, &mut i, 10.0, 2.0, 10.0, 2);
        let areas = body_areas(&v, &i, &[true, true]);
        assert!((areas[0] - 120.0).abs() < 1e-3, "{areas:?}");
        assert!((areas[1] - 120.0).abs() < 1e-3, "{areas:?}");
    }

    #[test]
    fn separate_bodies_and_non_water_stay_apart() {
        let (mut v, mut i) = (Vec::new(), Vec::new());
        quad(&mut v, &mut i, 0.0, 10.0, 10.0, 1);
        quad(&mut v, &mut i, 20.0, 3.0, 3.0, 2);
        // Ground touching the small pool is not water.
        quad(&mut v, &mut i, 23.0, 50.0, 50.0, 3);
        let areas = body_areas(&v, &i, &[true, true, false]);
        assert!((areas[0] - 100.0).abs() < 1e-3, "{areas:?}");
        assert!((areas[1] - 9.0).abs() < 1e-3, "{areas:?}");
        assert_eq!(areas[2], 0.0);
    }
}
