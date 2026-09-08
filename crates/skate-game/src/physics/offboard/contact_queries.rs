//! Scene boundary for the recovered BipedToolkit probe layout. The pending
//! packet owns observations; the world's geometry and acceleration tree stay shared.
use skate_core::{
    math::Vector3,
    physics::{
        board_world::{BoardWorld, query_metadata::QueryPool},
        drive_frames::RetailAffineTransform,
        triangle_query::{TriangleLineHit, triangle_segment},
    },
    player::offboard::contact_queries::{Input, Layout, Probe},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    pub geometry: TriangleLineHit,
    pub packed_surface: u16,
    pub mesh_index: usize,
    pub support_frame: RetailAffineTransform,
}

pub(crate) struct Completed {
    pub input: Input,
    pub support: [Option<Hit>; 3],
    pub obstacles: Vec<Option<Hit>>,
}

/// Connect actual canonical scene observations to the recovered classifier.
pub(crate) fn submit_toolkit(
    world: &BoardWorld,
    layout: &Layout,
    input: Input,
    matching_id: u32,
) -> Result<skate_core::player::offboard::contact_toolkit::Observations, String> {
    use skate_core::player::offboard::{
        contact_packet::SupportHit, contact_toolkit, ground_query::Edge,
    };
    let completed = submit(world, layout, input, matching_id)?;
    let v = |p: Vector3| [p.x, p.y, p.z, 0.];
    let support = completed.support.map(|h| {
        h.map(|h| {
            let f = h.support_frame;
            let b = f.basis.columns;
            SupportHit {
                position: v(h.geometry.position),
                normal: v(h.geometry.normal),
                frame: [
                    [b[0][0], b[0][1], b[0][2], 0.],
                    [b[1][0], b[1][1], b[1][2], 0.],
                    [b[2][0], b[2][1], b[2][2], 0.],
                    v(f.translation),
                ],
                // Guest query136 identifies the supporting body. Canonical mesh
                // identities replace guest pointers in this authored static scene.
                support_id: u32::try_from(h.mesh_index + 1)
                    .expect("canonical map mesh identity fits u32"),
            }
        })
    });
    let obstacles: Vec<_> = completed
        .obstacles
        .into_iter()
        .map(|h| {
            h.map(|h| contact_toolkit::Hit {
                position: v(h.geometry.position),
                normal: v(h.geometry.normal),
            })
        })
        .collect();
    let obstacles = obstacles.try_into().map_err(|_: Vec<_>| {
        "Biped query provider did not complete all44 native query slots".to_owned()
    })?;
    let metadata = world.query_metadata().map_err(str::to_owned)?;
    let (min, max) = contact_toolkit::edge_bounds(input);
    let bounds = skate_core::physics::board_world::query_metadata::Bounds {
        min: xyz(min),
        max: xyz(max),
    };
    // This BoardWorld owns an authored static scene; it has no dynamic/vehicle
    // edge providers. Preserve static provider order and the native shared cap.
    let edges = metadata
        .static_edges
        .iter()
        .filter(|edge| edge.local_bounds.overlaps(bounds))
        .take(40)
        .map(|edge| Edge {
            start: edge.start,
            end: edge.end,
        })
        .collect();
    Ok(contact_toolkit::Observations {
        input,
        support,
        obstacles,
        edges,
    })
}

pub(crate) fn submit(
    world: &BoardWorld,
    layout: &Layout,
    input: Input,
    matching_id: u32,
) -> Result<Completed, String> {
    let (support, obstacles) = layout.world_probes(input);
    let support = support.map(|probe| query(world, probe, matching_id));
    let [a, b, c] = support;
    let mut hits = vec![None; obstacles.iter().map(|p| p.id + 1).max().unwrap_or(0)];
    for probe in obstacles {
        hits[probe.id] = query(world, probe, matching_id)?;
    }
    Ok(Completed {
        input,
        support: [a?, b?, c?],
        obstacles: hits,
    })
}

fn xyz(v: [f32; 4]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}
pub(crate) fn query(world: &BoardWorld, probe: Probe, matching_id: u32) -> Result<Option<Hit>, String> {
    let metadata = world.query_metadata().map_err(str::to_owned)?;
    let start = xyz(probe.start);
    let end = xyz(probe.end);
    let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
    let mut nearest: Option<Hit> = None;
    // Accelerate the canonical identity-transform map representation. Authored
    // transformed meshes retain the same geometry path without incorrect culling.
    let candidates: Vec<_> = if metadata
        .meshes
        .iter()
        .all(|m| m.local_to_world == RetailAffineTransform::IDENTITY)
    {
        world
            .line_candidates(start, end, probe.radius)
            .map(|(i, _)| i)
            .collect()
    } else {
        (0..world.triangles().len()).collect()
    };
    for pool in [QueryPool::Ground, QueryPool::Island, QueryPool::Conditional] {
        if pool == QueryPool::Conditional && metadata.island_flags != 3 {
            continue;
        }
        for (mesh_index, mesh) in metadata.meshes.iter().enumerate() {
            if mesh.pool != pool
                || !(mesh.matching_group == -1 || mesh.matching_group == matching_id as i32)
            {
                continue;
            }
            let first = candidates.partition_point(|&i| i < mesh.triangle_range.start);
            let last = candidates.partition_point(|&i| i < mesh.triangle_range.end);
            for &index in &candidates[first..last] {
                let triangle = &world.triangles()[index].triangle;
                let frame = mesh.local_to_world;
                let vertices = triangle.vertices.map(|v| {
                    let t = frame.translation;
                    let b = frame.basis.columns;
                    Vector3::new(
                        b[2][0].mul_add(v.z, b[1][0].mul_add(v.y, b[0][0].mul_add(v.x, t.x))),
                        b[2][1].mul_add(v.z, b[1][1].mul_add(v.y, b[0][1].mul_add(v.x, t.y))),
                        b[2][2].mul_add(v.z, b[1][2].mul_add(v.y, b[0][2].mul_add(v.x, t.z))),
                    )
                });
                let mut geometry = TriangleLineHit {
                    position: Vector3::ZERO,
                    normal: Vector3::ZERO,
                    fraction: 0.,
                    volume_parameter: [0.; 3],
                };
                if triangle_segment(
                    &mut geometry,
                    start,
                    delta,
                    vertices,
                    probe.radius,
                    triangle.fatness,
                ) && nearest
                    .as_ref()
                    .is_none_or(|h| geometry.fraction < h.geometry.fraction)
                {
                    nearest = Some(Hit {
                        geometry,
                        packed_surface: metadata.packed_surfaces[index],
                        mesh_index,
                        support_frame: frame,
                    });
                }
            }
        }
    }
    Ok(nearest)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_floor_and_empty_space_produce_real_support_observations() {
        let world =
            crate::physics::ground::world(skate_core::physics::contact::RetailContactMaterial {
                static_friction: 0.,
                dynamic_friction: 0.,
                restitution: 0.,
            });
        let mut input = Input {
            position: [0., 0., 0., 0.],
            surface_right: [1., 0., 0., 0.],
            surface_up: [0., 1., 0., 0.],
            surface_forward: [0., 0., 1., 0.],
            animation_right: [1., 0., 0., 0.],
            animation_up: [0., 1., 0., 0.],
            velocity: [0.; 4],
        };
        let layout = Layout::default();
        let hits = submit(&world, &layout, input, 0).unwrap();
        assert!(hits.support.iter().all(Option::is_some));
        assert!(hits.obstacles.iter().any(Option::is_some));
        assert_eq!(hits.support[0].unwrap().packed_surface, 0);
        input.position[0] = 1000.;
        let hits = submit(&world, &layout, input, 0).unwrap();
        assert!(hits.support.iter().all(Option::is_none));
        assert!(hits.obstacles.iter().all(Option::is_none));
    }
    #[test]
    fn real_floor_queries_run_through_the_complete_contact_classifier() {
        use skate_core::player::offboard::contact_toolkit::Toolkit;
        let world =
            crate::physics::ground::world(skate_core::physics::contact::RetailContactMaterial {
                static_friction: 0.,
                dynamic_friction: 0.,
                restitution: 0.,
            });
        let layout = Layout::default();
        let mut toolkit = Toolkit::default();
        for speed in [0., 1., 3., 6.] {
            let input = Input {
                position: [0.; 4],
                surface_right: [1., 0., 0., 0.],
                surface_up: [0., 1., 0., 0.],
                surface_forward: [0., 0., 1., 0.],
                animation_right: [1., 0., 0., 0.],
                animation_up: [0., 1., 0., 0.],
                velocity: [0., 0., speed, 0.],
            };
            toolkit.submit(submit_toolkit(&world, &layout, input, 0).unwrap());
            toolkit.refresh(&layout);
            assert_eq!(
                toolkit.packet.flags & 3,
                3,
                "speed{speed}: {:?}",
                toolkit.packet
            );
            assert!(toolkit.packet.target_position.iter().all(|v| v.is_finite()));
            assert_eq!(toolkit.packet.kind_164, 0);
            assert!(toolkit.pending.is_none());
            assert_eq!(toolkit.contact_age, 30);
        }
    }
}
