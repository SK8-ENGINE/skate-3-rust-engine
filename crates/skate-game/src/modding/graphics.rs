//! Package-scoped graphics and immutable-bind-pose node overrides. All physics
//! and gameplay stay outside this module. A scene node is not a vehicle part.
use super::Mods;
use bevy::{prelude::*, scene::{SceneInstance,SceneSpawner}};
use skate_mods::scene::{GraphicsDefinition, NodeState, TransformOptions, TransformState};
use std::{collections::{BTreeMap,BTreeSet},time::Instant};

#[derive(Clone)]
pub(super) struct TimedNode { pub state:NodeState, pub received:Instant }
#[derive(Clone,Copy)]
struct Binding { entity:Entity, authored:Transform }
pub(super) struct Owned {
    pub entity:Entity,
    pub mesh:Option<AssetId<Mesh>>,
    pub material:Option<AssetId<StandardMaterial>>,
    pub body:Option<String>,
    pub definition:GraphicsDefinition,
    pub transform:TransformState,
    pub visible:bool,
    pub serial:u64,
    pub nodes:BTreeMap<String,TimedNode>,
    bindings:BTreeMap<String,Binding>,
    ambiguous:BTreeSet<String>,
    warned:BTreeSet<String>,
    ready:bool,
}

pub(super) fn transform(state:&TransformState) -> Transform {
    Transform {
        translation:Vec3::from_array(state.position),
        rotation:Quat::from_array(state.rotation).normalize(),
        scale:Vec3::from_array(state.scale),
    }
}
pub(super) fn node_transform(authored:Transform, state:&NodeState, age:f32) -> Transform {
    let mut delta=transform(&state.transform);
    let age=age.clamp(0.,0.10);
    delta.translation += Vec3::from_array(state.linear_velocity)*age;
    delta.rotation=(Quat::from_scaled_axis(Vec3::from_array(state.angular_velocity)*age)*delta.rotation).normalize();
    if state.relative {
        Transform { translation:authored.translation+delta.translation,
            rotation:(delta.rotation*authored.rotation).normalize(),scale:authored.scale*delta.scale }
    } else { delta }
}

/// Canonical containment defeats symlink escapes as well as lexical traversal.
/// Remote peers supply only a relative descriptor in a matching local package.
pub(super) fn asset_path(mods:&Mods, package_id:&str, path:&str) -> Result<String,String> {
    if !skate_mods::scene::valid_asset(path) || path.is_empty() { return Err("invalid GLB asset path".into()); }
    let package=mods.manager.packages.get(package_id).ok_or("missing local package")?;
    let root=package.root.canonicalize().map_err(|e|format!("package path: {e}"))?;
    let full=root.join(path).canonicalize().map_err(|e|format!("GLB {path}: {e}"))?;
    if !full.starts_with(&root) || !full.is_file() { return Err("GLB escapes its package".into()); }
    let reader_root=super::package_root().canonicalize().map_err(|e|e.to_string())?;
    let relative=full.strip_prefix(&reader_root).map_err(|_|"GLB outside mods asset reader")?;
    Ok(format!("mods://{}",relative.to_string_lossy().replace('\\',"/")))
}

pub(super) fn spawn(
    world:&mut World, mods:&mut Mods, owner:&str, package:&str, key:String,
    definition:GraphicsDefinition, state:TransformState, visible:bool, serial:Option<u64>,
) -> Result<(),String> {
    if !definition.validate() || !state.validate() || !skate_mods::scene::valid_key(&key) {
        return Err("invalid graphics descriptor or transform".into());
    }
    let slot=(owner.to_owned(),key);
    if !mods.graphics.contains_key(&slot) && mods.graphics.keys().filter(|(o,_)| o==owner).count() >= 64 {
        return Err("64 graphics instances per owner maximum".into());
    }
    // Validate before removing a live instance. An async GLB load is never a
    // license to fall back to an arbitrary path or another package's scene.
    let path=if definition.path.is_empty() { None } else { Some(asset_path(mods,package,&definition.path)?) };
    super::retire_graphics(world,mods,&slot);
    let t=transform(&state);
    let visibility=if visible { Visibility::Visible } else { Visibility::Hidden };
    let (entity,mesh,material)=if let Some(path)=path {
        let scene=world.resource::<AssetServer>().load(GltfAssetLabel::Scene(0).from_asset(path));
        (world.spawn((SceneRoot(scene),t,visibility)).id(),None,None)
    } else {
        let mesh=world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(1.,1.,1.));
        let c=definition.color;
        let material=world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color:Color::srgb(c[0],c[1],c[2]),..default()
        });
        (world.spawn((Mesh3d(mesh.clone()),MeshMaterial3d(material.clone()),t,visibility)).id(),Some(mesh.id()),Some(material.id()))
    };
    mods.graphics_serial=mods.graphics_serial.wrapping_add(1);
    world.entity_mut(entity).insert(crate::retail_character::ModGraphicsLit);
    mods.graphics.insert(slot,Owned {
        entity,mesh,material,body:definition.body.clone(),definition,transform:state,visible,
        serial:serial.unwrap_or(mods.graphics_serial),nodes:BTreeMap::new(),bindings:BTreeMap::new(),
        ambiguous:BTreeSet::new(),warned:BTreeSet::new(),ready:false,
    });
    Ok(())
}
pub(super) fn set_node(mods:&mut Mods,owner:&str,key:&str,node:String,options:TransformOptions) -> Result<(),String> {
    let owned=mods.graphics.get_mut(&(owner.to_owned(),key.to_owned())).ok_or("unknown graphics key")?;
    if owned.nodes.len() >= 64 && !owned.nodes.contains_key(&node) { return Err("64 node overrides per graphics instance maximum".into()); }
    let frame=owned.nodes.entry(node).or_insert_with(||TimedNode { state:NodeState::default(),received:Instant::now() });
    frame.state.apply(&options); frame.received=Instant::now();
    Ok(())
}
pub(super) fn reset_node(world:&mut World,mods:&mut Mods,owner:&str,key:&str,node:&str) {
    if let Some(owned)=mods.graphics.get_mut(&(owner.to_owned(),key.to_owned())) {
        owned.nodes.remove(node);
        if let Some(b)=owned.bindings.get(node) {
            if let Some(mut t)=world.get_mut::<Transform>(b.entity) { *t=b.authored; }
        }
    }
}

fn bind(world:&World,owned:&mut Owned) {
    if owned.ready { return; }
    if owned.definition.path.is_empty() { owned.ready=true; return; }
    let Some(instance)=world.get::<SceneInstance>(owned.entity) else { return };
    let spawner=world.resource::<SceneSpawner>();
    if !spawner.instance_is_ready(**instance) { return; }
    for entity in spawner.iter_instance_entities(**instance) {
        let (Some(name),Some(t))=(world.get::<Name>(entity),world.get::<Transform>(entity)) else { continue };
        let name=name.as_str().to_owned();
        if owned.bindings.insert(name.clone(),Binding { entity,authored:*t }).is_some() {
            owned.ambiguous.insert(name);
        }
    }
    owned.ready=true;
}

pub(super) fn sync(world:&mut World,mods:&mut Mods) {
    for ((owner,key),owned) in &mut mods.graphics {
        let mut t=transform(&owned.transform);
        let mut visible=owned.visible;
        if let Some(body)=&owned.body {
            if let Some(snap)=mods.bodies.get(&(owner.clone(),body.clone())).and_then(|id|mods.world.read(*id)) {
                let q=Quat::from_array(snap.rotation).normalize();
                t.translation=Vec3::from_array(snap.position)+q*t.translation;
                t.rotation=(q*t.rotation).normalize();
            } else {
                // Out-of-order network spawn: never show a body-bound scene
                // at the origin while its validated collider is still pending.
                visible=false;
            }
        }
        if let Some(mut current)=world.get_mut::<Transform>(owned.entity) { *current=t; }
        if let Some(mut current)=world.get_mut::<Visibility>(owned.entity) { *current=if visible { Visibility::Visible } else { Visibility::Hidden }; }
        bind(world,owned);
        for (name,node) in &owned.nodes {
            if owned.ambiguous.contains(name) || !owned.bindings.contains_key(name) {
                if owned.ready && owned.warned.insert(name.clone()) {
                    warn!("graphics {owner}/{key}: node '{name}' missing or ambiguous in local GLB");
                }
                continue;
            }
            let binding=owned.bindings[name];
            let age=if owner.starts_with('@') { node.received.elapsed().as_secs_f32() } else { 0. };
            if let Some(mut current)=world.get_mut::<Transform>(binding.entity) {
                *current=node_transform(binding.authored,&node.state,age);
            }
        }
    }
}

/// Actual current solid collider edges, not the mesh or an approximate box.
/// Green=local; amber=remote replica; cyan=physical center of mass.
pub(crate) fn debug(mods:Res<Mods>,mut gizmos:Gizmos) {
    if mods.debug_owners.is_empty() { return; }
    let mut budget=60_000;
    for body in mods.world.solid_bodies() {
        let Some(((owner,_),_))=mods.bodies.iter().find(|(_,id)| **id==body.id) else { continue };
        let package=owner.split_once(':').map_or(owner.as_str(),|(_,id)|id);
        if !mods.debug_owners.contains(owner) && !mods.debug_owners.contains(package) { continue; }
        let color=if owner.starts_with('@') { Color::srgb(1.,0.65,0.1) } else { Color::srgb(0.1,1.,0.25) };
        // Shared triangle edges need only one line. This keeps detailed
        // compounds visible without spending the line budget twice per edge.
        let mut edges=BTreeSet::new();
        for collider in &body.colliders {
            for triangle in skate_dynamics::solid::collider_triangles(collider) {
                for (a,b) in [(0,1),(1,2),(2,0)] {
                    let mut first=triangle[a].map(f32::to_bits);
                    let mut second=triangle[b].map(f32::to_bits);
                    if first>second { std::mem::swap(&mut first,&mut second); }
                    if !edges.insert((first,second)) { continue; }
                    if budget==0 { return; } budget-=1;
                    gizmos.line(Vec3::from_array(triangle[a]),Vec3::from_array(triangle[b]),color);
                }
            }
        }
        let com=Vec3::from_array(body.center_of_mass.to_array());
        for axis in [Vec3::X,Vec3::Y,Vec3::Z] {
            gizmos.line(com-axis*0.12,com+axis*0.12,Color::srgb(0.,1.,1.));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn bind_delta_does_not_accumulate_or_discard_authored_scale() {
        let authored=Transform::from_xyz(2.,3.,4.).with_scale(Vec3::splat(2.));
        let mut state=NodeState::default(); state.transform.position=[0.,0.2,0.];
        state.transform.rotation=Quat::from_rotation_y(0.4).to_array();
        let a=node_transform(authored,&state,0.);
        let b=node_transform(authored,&state,0.);
        assert_eq!(a,b); assert_eq!(a.scale,Vec3::splat(2.));
        assert!((a.translation.y-3.2).abs()<1e-5);
    }
    #[test] fn node_extrapolation_freezes_at_one_tenth_second() {
        let mut state=NodeState::default();state.angular_velocity=[20.,0.,0.];
        assert_eq!(node_transform(Transform::IDENTITY,&state,1.),node_transform(Transform::IDENTITY,&state,0.1));
    }
}
