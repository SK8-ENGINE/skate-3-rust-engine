fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sdk/examples/skyline/skyline.glb");
    let (doc, buffers, _) = gltf::import(&path).expect("load skyline.glb");
    for node in doc.nodes().filter(|node| {
        matches!(
            node.name(),
            Some("skyline_mesh" | "wheel_fl" | "wheel_fr" | "wheel_rl" | "wheel_rr")
        )
    }) {
        let (translation, rotation, scale) = node.transform().decomposed();
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
                if let Some(positions) = reader.read_positions() {
                    for position in positions {
                        for axis in 0..3 {
                            min[axis] = min[axis].min(position[axis]);
                            max[axis] = max[axis].max(position[axis]);
                        }
                    }
                }
            }
        }
        println!(
            "{} translation={translation:?} rotation={rotation:?} scale={scale:?} mesh_bounds={min:?}..{max:?}",
            node.name().unwrap_or("<unnamed>")
        );
        print_children(node, 1);
    }
    for name in ["skyline_mesh", "wheel_fl", "wheel_fr", "wheel_rl", "wheel_rr"] {
        let points = skate_mods::convex_points_file(&path, name).expect("convex points");
        let min = std::array::from_fn::<_, 3, _>(|axis| {
            points
                .iter()
                .map(|point| point[axis])
                .fold(f32::MAX, f32::min)
        });
        let max = std::array::from_fn::<_, 3, _>(|axis| {
            points
                .iter()
                .map(|point| point[axis])
                .fold(f32::MIN, f32::max)
        });
        println!("{name} local_hull={min:?}..{max:?} count={}", points.len());
    }
}

fn print_children(node: gltf::Node<'_>, depth: usize) {
    for child in node.children() {
        let (translation, rotation, scale) = child.transform().decomposed();
        println!(
            "{}{} mesh={:?} translation={translation:?} rotation={rotation:?} scale={scale:?}",
            "  ".repeat(depth),
            child.name().unwrap_or("<unnamed>"),
            child.mesh().and_then(|mesh| mesh.name()),
        );
        print_children(child, depth + 1);
    }
}
