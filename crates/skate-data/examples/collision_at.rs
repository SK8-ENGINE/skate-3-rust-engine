//! Every retail collision triangle of a map under a point: its height, stream and surface (is there ground where a
//! teleport fell through?). Data only. usage: collision_at <Map.skate> <x> <z>
fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("Expected a .skate map")?;
    let x: f32 = args.next().ok_or("Expected x")?.parse().map_err(|e| format!("x: {e}"))?;
    let z: f32 = args.next().ok_or("Expected z")?.parse().map_err(|e| format!("z: {e}"))?;
    let map = skate_data::skate_map::SkateMap::load(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    let archive = map.extensions.iter().find(|e| e.tag == *b"RWCM").ok_or("no RWCM collision")?;
    let mut hits = Vec::new();
    let mut near = f32::MAX;
    skate_data::retail_collision::visit_clusters(&archive.payload, |name, cluster| {
        for t in cluster {
            let [a, b, c] = t.points;
            let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
            if d.abs() < 1e-9 {
                continue;
            }
            let u = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
            let v = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
            let w = 1. - u - v;
            if u.min(v).min(w) >= -1e-6 {
                hits.push((u * a[1] + v * b[1] + w * c[1], name.to_owned(), t.surface));
            }
            for p in t.points {
                near = near.min(((p[0] - x).powi(2) + (p[2] - z).powi(2)).sqrt());
            }
        }
        Ok(())
    })?;
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("{} triangles under ({x}, {z}); nearest vertex {near:.2} m away", hits.len());
    for (h, name, surface) in hits.iter().take(20) {
        println!("  y {h:.2}  surface {surface}  {name}");
    }
    Ok(())
}
