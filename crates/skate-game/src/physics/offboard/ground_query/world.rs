//! Ephemeral adapter over the canonical BoardWorld metadata. Geometry is borrowed.
use super::{Bounds,Frame,Mesh,PrimaryEdges,Scene,Segment,IndexedEdgeBody};
use skate_core::{math::Vector3,physics::{board_world::{BoardWorld,query_metadata::QueryPool},drive_frames::RetailAffineTransform},player::offboard::ground_query::Edge};

/// Caller supplies actual dynamic-provider views from the same live scene owner.
/// Empty providers mean the authored scene actually has none, never a failed lookup.
pub fn with_world_scene<T>(
    world:&BoardWorld,
    primary_edges:PrimaryEdges<'_>,
    indexed_edges:&[IndexedEdgeBody<'_>],
    operation:impl FnOnce(&mut Scene<'_>)->Result<T,&'static str>,
)->Result<T,&'static str> {
    let metadata=world.query_metadata()?;
    let triangles=world.triangles();
    let mut pools:[Vec<Mesh<'_>>;3]=std::array::from_fn(|_|Vec::new());
    for source in &metadata.meshes {
        let index=match source.pool {QueryPool::Ground=>0,QueryPool::Island=>1,QueryPool::Conditional=>2};
        pools[index].push(Mesh {
            local_to_world:frame(source.local_to_world),world_to_local:frame(source.world_to_local),
            local_bounds:Bounds {min:source.local_bounds.min,max:source.local_bounds.max},
            matching_group:source.matching_group,
            triangles:&triangles[source.triangle_range.clone()],
            surfaces:&metadata.packed_surfaces[source.triangle_range.clone()],
        });
    }
    let edges:Vec<_>=metadata.static_edges.iter().map(|s|Segment {
        edge:Edge {start:s.start,end:s.end},local_bounds:Bounds {min:s.local_bounds.min,max:s.local_bounds.max},
    }).collect();
    let mut scene=Scene {ground_pool:&pools[0],island_pool:&pools[1],conditional_pool:&pools[2],
        island_flags:metadata.island_flags,static_edges:&edges,primary_edges,indexed_edges};
    operation(&mut scene)
}
fn frame(f:RetailAffineTransform)->Frame {
    let v=|a:[f32;3]|Vector3::new(a[0],a[1],a[2]);
    Frame {right:v(f.basis.columns[0]),up:v(f.basis.columns[1]),forward:v(f.basis.columns[2]),position:f.translation}
}
