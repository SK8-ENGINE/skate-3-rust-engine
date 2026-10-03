//! Manual actual-asset CPU probe. Does not launch a game or imply an FPS result.
use skate_dynamics::{BodyDesc,BodyType,DynamicsWorld,Shape,deformation::Options,definition_codec};
use std::{path::PathBuf,time::Instant};
fn run()->Result<(),String> {
    let path=PathBuf::from(std::env::args().nth(1).ok_or("GLB path required")?);
    let shape=skate_mods::model_shape_file(&path,"skyline_mesh",&Default::default())?;
    let geometry=skate_mods::model::read_geometry_file(&path,"skyline_mesh",&Default::default())?;
    let max_z=geometry.vertices.iter().map(|p|p[2]).fold(f32::MIN,f32::max);
    for (label,width) in [("wall",8.),("post",0.16)] {
        let mut w=DynamicsWorld::default();
        w.spawn(BodyDesc {body_type:BodyType::Static,shape:Shape::Box {half_extents:[width,8.,0.25]},
            position:[0.,20.,max_z+0.5],..Default::default()})?;
        let t=Instant::now();
        let id=w.spawn(BodyDesc {shape:shape.clone(),mass:1400.,position:[0.,20.,0.],ccd:true,
            linear_damping:0.,angular_damping:0.,deformation:Some(Options::default()),
            center_of_mass:[0.,-0.27,0.16],inertia_half_extents:Some([0.92,0.55,2.1]),..Default::default()})?;
        println!("{label} deformation setup {:?}",t.elapsed());
        w.set_linvel(id,[0.,0.,20.]);
        let mut micros=Vec::new();let mut damages=Vec::new();let mut revision=0;
        for _ in 0..240 {
            let t=Instant::now();w.step(1./120.);let us=t.elapsed().as_secs_f64()*1e6;
            let now=w.deformation(id).unwrap().revision;
            if now!=revision {damages.push(us);revision=now;}
            micros.push(us);
        }
        micros.sort_by(f64::total_cmp);
        if revision==0 {return Err(format!("actual {label} crash did not deform"));}
        let definition=w.body_definition(id).unwrap();let bytes=definition_codec::encode(&definition)?;
        let decoded=definition_codec::decode(&bytes)?;
        let mut replica=DynamicsWorld::default();let remote=replica.spawn_replica(&decoded)?;
        if replica.deformation(remote).unwrap().offsets!=w.deformation(id).unwrap().offsets {return Err("replica mesh mismatch".into());}
        println!("{label}: revisions={revision}, damage_ticks_us={damages:?}, physics_tick_p99_us={:.1}, max_us={:.1}, definition_bytes={}",
            micros[237],micros[239],bytes.len());
    }
    println!("PASS actual Skyline model contacts, deformed solid export and replica field. Does not measure rendering or online latency.");
    Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
