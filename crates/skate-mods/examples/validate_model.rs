//! Exercise the real GLB reader, Parry cook, Rapier shape and replication codec.
//! Writes no collision assets. Usage: validate_model <render.glb> [node-or-mesh]
use skate_dynamics::{BodyDesc, DynamicsWorld, ModelColliderOptions, Shape, definition_codec};
use std::{path::PathBuf, time::Instant};

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(args.next().ok_or("usage: validate_model <render.glb> [node-or-mesh]")?);
    let object = args.next().map(|s| s.into_string().map_err(|_| "selector must be UTF-8"))
        .transpose()?.unwrap_or_default();
    if args.next().is_some() { return Err("too many arguments".into()); }
    let options = ModelColliderOptions::default();
    let geometry = skate_mods::model::read_geometry_file(&path, &object, &options)?;
    let start = Instant::now();
    let shape = skate_mods::model_shape_file(&path, &object, &options)?;
    let cook_time = start.elapsed();
    let Shape::Compound { hulls } = &shape else { return Err("expected compound model collision".into()) };
    let mut local = DynamicsWorld::default();
    let id = local.spawn(BodyDesc { shape: shape.clone(), mass: 100., ccd: true, ..Default::default() })?;
    let definition = local.body_definition(id).ok_or("missing definition")?;
    let bytes = definition_codec::encode(&definition)?;
    let decoded = definition_codec::decode(&bytes)?;
    let mut remote = DynamicsWorld::default();
    let remote_id = remote.spawn_replica(&decoded)?;
    if remote.body_definition(remote_id).ok_or("missing replica")?.body.shape != shape {
        return Err("replica geometry differs from local body".into());
    }
    let local_solids = local.solid_bodies();
    let remote_solids = remote.solid_bodies();
    if local_solids.len() != 1 || remote_solids.len() != 1
        || local_solids[0].colliders.len() != hulls.len()
        || remote_solids[0].colliders.len() != hulls.len() {
        return Err("compound lost its single-body identity or native solid parts".into());
    }
    if (local_solids[0].inverse_mass - 1. / 100.).abs() > 1e-6
        || (remote_solids[0].inverse_mass - local_solids[0].inverse_mass).abs() > 1e-6 {
        return Err("compound mass was applied per part rather than once".into());
    }
    let cached_start = Instant::now();
    if skate_mods::model_shape_file(&path, &object, &options)? != shape { return Err("cache mismatch".into()); }
    println!("Render GLB: {}\nSelector: {:?}\nSource vertices (position welded): {}\nSource triangles: {}",
        path.display(), object, geometry.vertices.len(), geometry.triangles.len());
    println!("Convex parts: {}\nFinal hull vertices: {}\nNetwork definition: {} bytes\nFirst cook: {:?}\nCached read: {:?}",
        hulls.len(), hulls.iter().map(Vec::len).sum::<usize>(), bytes.len(), cook_time, cached_start.elapsed());
    println!("PASS: actual cook/spawn/replica/one-mass/solid-parts/cache. No collision file written.\nThis is not a BoardWorld, walking or multiplayer session test.");
    Ok(())
}
fn main() {
    if let Err(error) = run() { eprintln!("MODEL VALIDATION FAILED: {error}"); std::process::exit(1); }
}
