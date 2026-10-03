//! Instance-local GLB deformation. Authored materials/UVs remain intact.
use bevy::{prelude::*, mesh::{VertexAttributeValues, PrimitiveTopology}};
use skate_dynamics::deformation::Field;

#[derive(Default)]
pub(super) struct State {
    revision: Option<(u64,u64)>,
    parts: Vec<Part>,
}
struct Part {
    entity: Entity,
    mesh: Handle<Mesh>,
    rest: Vec<[f32;3]>,
    previous: Vec<[f32;3]>,
    private: bool,
    weights: Vec<([usize;8],[f32;8])>,
    to_mesh: Mat4,
}
impl State {
    pub(super) fn clear(self, world:&mut World) {
        let mut meshes=world.resource_mut::<Assets<Mesh>>();
        for part in self.parts {if part.private {meshes.remove(part.mesh.id());}}
    }
}

fn binding(world:&World, root:Entity, entity:Entity, names:&[String]) -> Option<Mat4> {
    let mut current=entity;
    let mut matrix=Mat4::IDENTITY;
    let mut selected=false;
    // Only traverse this scene; a GLB name can never bind another mod's mesh.
    for _ in 0..128 {
        if current==root {return selected.then_some(matrix);}
        selected |= world.get::<Name>(current).is_some_and(|n|names.iter().any(|s|s==n.as_str()));
        matrix=world.get::<Transform>(current)?.to_matrix()*matrix;
        current=world.get::<ChildOf>(current)?.parent();
    }
    None
}

pub(super) fn sync(world:&mut World, owned:&mut super::graphics::Owned,
    id:u64, field:&Field, offset:[f32;3]) {
    if owned.definition.deform_nodes.is_empty() || !owned.ready
        || owned.deformation.revision==Some((id,field.revision)) {return;}
    // Don't clone/upload pristine meshes. Only a real damage revision allocates.
    if field.revision==0 && owned.deformation.parts.is_empty() {
        owned.deformation.revision=Some((id,0)); return;
    }
    if owned.deformation.parts.is_empty() {
        let mut entities=Vec::new();let mut pending=vec![owned.entity];
        while let Some(e)=pending.pop() {
            if let Some(children)=world.get::<Children>(e) {pending.extend(children.iter());}
            if world.get::<Mesh3d>(e).is_some() {entities.push(e);}
        }
        let root=super::graphics::transform(&owned.transform).to_matrix();
        let mut total=0usize;
        for entity in entities {
            let Some(local)=binding(world,owned.entity,entity,&owned.definition.deform_nodes) else {continue};
            let to_body=Mat4::from_translation(-Vec3::from_array(offset))*root*local;
            if !to_body.is_finite() || to_body.determinant().abs()<1e-8 {continue;}
            let Some(handle)=world.get::<Mesh3d>(entity).map(|m|m.0.clone()) else {continue};
            let Some(mesh)=world.resource::<Assets<Mesh>>().get(&handle) else {continue};
            if mesh.primitive_topology()!=PrimitiveTopology::TriangleList {continue;}
            let Some(VertexAttributeValues::Float32x3(positions))=mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {continue};
            total+=positions.len();
            if total>262_144 {warn!("deformation instance exceeds 262144 vertices");break;}
            let rest=positions.clone();
            let weights=rest.iter().map(|p|field.weights(to_body.transform_point3(Vec3::from_array(*p)).to_array())).collect();
            owned.deformation.parts.push(Part {entity,mesh:handle,previous:rest.clone(),private:false,rest,weights,to_mesh:to_body.inverse()});
        }
        if owned.deformation.parts.is_empty() {
            warn!("deformation binding matched no readable GLB triangle meshes");
        }
    }
    if owned.deformation.revision.is_some_and(|(previous,_)|previous!=id) {
        for part in &mut owned.deformation.parts {
            let to_body=part.to_mesh.inverse();
            part.weights=part.rest.iter().map(|p|field.weights(to_body.transform_point3(Vec3::from_array(*p)).to_array())).collect();
        }
    }
    for part in &mut owned.deformation.parts {
        let positions:Vec<_>=part.rest.iter().zip(&part.weights).map(|(rest,(ids,weights))| {
            let displacement=(0..8).fold(Vec3::ZERO,|v,i|v+Vec3::from_array(field.offsets[ids[i]])*weights[i]);
            (Vec3::from_array(*rest)+part.to_mesh.transform_vector3(displacement)).to_array()
        }).collect();
        if positions.iter().zip(&part.previous).all(|(a,b)|Vec3::from_array(*a).distance_squared(Vec3::from_array(*b))<1e-10) {continue;}
        if !part.private {
            let Some(clone)=world.resource::<Assets<Mesh>>().get(&part.mesh).cloned() else {continue};
            part.mesh=world.resource_mut::<Assets<Mesh>>().add(clone);part.private=true;
            world.entity_mut(part.entity).insert(Mesh3d(part.mesh.clone()));
        }
        part.previous=positions.clone();
        let mut meshes=world.resource_mut::<Assets<Mesh>>();
        let Some(mesh)=meshes.get_mut(&part.mesh) else {continue};
        let tangents=mesh.contains_attribute(Mesh::ATTRIBUTE_TANGENT);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION,positions);
        mesh.compute_normals();
        if tangents {let _=mesh.generate_tangents();}
        drop(meshes);
        // Bevy recalculates the culling bounds; old bounds must not clip dents.
        world.entity_mut(part.entity).remove::<bevy::camera::primitives::Aabb>();
    }
    owned.deformation.revision=Some((id,field.revision));
}
