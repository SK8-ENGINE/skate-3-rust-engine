fn main() {
    let mut failed = false;
    for path in std::env::args_os().skip(1) {
        match skate_data::skate_map::SkateMap::load(std::path::Path::new(&path)) {
            Ok(m) => println!(
                "{} v{}: vertices={} triangles={} collision={} textures={} rails={} doors={} lights={} routes={} spawn={:?} heading={}",
                m.name,
                m.version,
                m.geometry.vertices.len(),
                m.geometry.indices.len() / 3,
                m.geometry.collision.len(),
                m.textures.len(),
                m.rails.len(),
                m.doors.len(),
                m.lights.len(),
                m.routes.len(),
                m.spawn,
                m.heading
            ),
            Err(e) => {
                eprintln!("{e}");
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
