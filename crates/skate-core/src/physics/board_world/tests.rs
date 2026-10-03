use super::*;
use crate::math::Basis3;
use crate::physics::drive_frames::RetailAffineTransform;
use query_metadata::{QueryMesh, QueryPool};

fn material() -> RetailContactMaterial {
    RetailContactMaterial {
        static_friction: 0.7,
        dynamic_friction: 0.4,
        restitution: 0.2,
    }
}
fn face(x: f32, tag: u32, fatness: f32) -> WorldTriangle {
    WorldTriangle::from_vertices(
        [
            Vector3::new(x - 2., 0., -2.),
            Vector3::new(x - 2., 0., 2.),
            Vector3::new(x + 2., 0., -2.),
        ],
        material(),
        tag,
        0xe0,
        [1.; 3],
        fatness,
    )
    .unwrap()
}
fn annotated(triangles: Vec<WorldTriangle>) -> BoardWorld {
    let meshes = triangles
        .iter()
        .enumerate()
        .map(|(i, t)| QueryMesh {
            triangle_range: i..i + 1,
            local_to_world: RetailAffineTransform::IDENTITY,
            world_to_local: RetailAffineTransform::IDENTITY,
            local_bounds: Bounds::from_points(t.triangle.vertices).unwrap(),
            matching_group: -1,
            rejection_flags: 0x2000,
            geometry: 5000 + i as u32,
            pool: QueryPool::Ground,
        })
        .collect();
    let packed_surfaces = vec![17; triangles.len()];
    BoardWorld::with_query_metadata(
        triangles,
        QueryMetadata {
            packed_surfaces,
            meshes,
            static_edges: vec![],
            island_flags: 0,
        },
    )
    .unwrap()
}
fn tiled() -> Vec<WorldTriangle> {
    (0..1024)
        .map(|i| face(((i * 37) % 1024) as f32 * 10., i, 0.))
        .collect()
}

#[test]
fn zip_hierarchy_matches_inclusive_linear_mesh_scan_and_source_order() {
    let world = annotated(tiled());
    let meshes = &world.query_metadata().unwrap().meshes;
    let index = query_index::QueryIndex::new(meshes);
    for x in [-100., -2., 0., 2., 8., 10., 155., 10230., 10300.] {
        for radius in [0., 2., 31., 20000.] {
            let bounds = Bounds::from_points([Vector3::new(x, 0., 0.)])
                .unwrap()
                .expanded(radius);
            let expected: Vec<_> = meshes
                .iter()
                .enumerate()
                .filter(|(_, m)| m.local_bounds.overlaps(bounds))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(index.query(bounds, meshes), expected);
        }
    }
}

#[test]
fn line_candidates_prune_distant_clusters_and_keep_canonical_indices() {
    let world = annotated(tiled());
    let start = Vector3::new(-1., 1., -1.);
    let end = Vector3::new(-1., -1., -1.);
    let indices: Vec<_> = world
        .line_candidates(start, end, 0.)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(indices, vec![0]);
    assert_eq!(world.query_metadata().unwrap().meshes[0].geometry, 5000);
    assert_eq!(
        world.query_metadata().unwrap().meshes[0].rejection_flags,
        0x2000
    );
    assert_eq!(world.query_metadata().unwrap().packed_surfaces[0], 17);
    assert_eq!(world.candidate_ranges(None), vec![0..1024]);
    let invalid = Bounds {
        min: Vector3::new(f32::NAN, 0., 0.),
        max: Vector3::ZERO,
    };
    assert_eq!(world.candidate_ranges(Some(invalid)), vec![0..1024]);
    let empty = annotated(vec![]);
    assert_eq!(empty.line_candidates(start, end, 0.).count(), 0);
}

#[test]
fn accelerated_lines_match_full_scan_with_fatness_and_equal_hits() {
    let mut triangles = tiled();
    triangles.insert(0, face(0., 9000, 0.1));
    triangles.insert(0, face(0., 9001, 0.1));
    let linear = BoardWorld::new(triangles.clone());
    let world = annotated(triangles);
    for x in [-100., -2.1, -1., 0., 2., 9., 509., 10229.] {
        for radius in [0., 0.03, 0.5] {
            let start = Vector3::new(x, 2., -1.);
            let end = Vector3::new(x, -2., -1.);
            assert_eq!(
                world.query_swept_line(start, end, radius),
                linear.query_swept_line(start, end, radius)
            );
        }
    }
    assert_eq!(
        world
            .query_thin_line(Vector3::new(-1., 2., -1.), Vector3::new(-1., -2., -1.))
            .unwrap()
            .unwrap()
            .tag,
        9001
    );
}

#[test]
fn thin_endpoint_and_barycentric_tolerances_survive_broadphase() {
    let triangles = vec![face(0., 7, 0.)];
    let linear = BoardWorld::new(triangles.clone());
    let world = annotated(triangles);
    for (start, end) in [
        (Vector3::new(-1., 1., -1.), Vector3::new(-1., 0.000005, -1.)),
        (
            Vector3::new(-1., -0.000005, -1.),
            Vector3::new(-1., -1., -1.),
        ),
        (
            Vector3::new(-2.00002, 1., -1.),
            Vector3::new(-2.00002, -1., -1.),
        ),
    ] {
        let expected = linear.query_thin_line(start, end).unwrap();
        assert!(
            expected.is_some(),
            "fixture must exercise the thin-leaf tolerance"
        );
        assert_eq!(world.query_thin_line(start, end).unwrap(), expected);
    }
}

#[test]
fn predictive_contacts_and_retention_match_full_scan_for_every_primitive() {
    let mut triangles = tiled();
    triangles.insert(0, face(0., 9000, 0.02));
    triangles.insert(0, face(0., 9001, 0.02));
    let mut linear = BoardWorld::new(triangles.clone());
    let mut world = annotated(triangles);
    let center = Vector3::new(-0.7, 0.25, -0.7);
    let primitives = [
        ContactPrimitive::Sphere(Sphere {
            center,
            radius: 0.2,
        }),
        ContactPrimitive::Capsule {
            center,
            axis: Vector3::new(1., 0., 0.),
            half_length: 0.3,
            radius: 0.2,
        },
        ContactPrimitive::RoundedBox {
            center,
            basis: Basis3 {
                columns: [[0.6, 0., 0.8], [0., 1., 0.], [-0.8, 0., 0.6]],
            },
            half_extents: Vector3::new(0.3, 0.2, 0.1),
            radius: 0.02,
        },
        ContactPrimitive::Triangle(triangle_from_volume(
            [
                Vector3::new(-1., 0.1, -1.),
                Vector3::new(-1., 0.1, 0.),
                Vector3::new(0., 0.1, -1.),
            ],
            0.03,
            [1.; 3],
            0xe0,
        )),
    ];
    let mut observed = false;
    for velocity in [-60., -1., 0., 1.] {
        let volumes: Vec<_> = primitives
            .iter()
            .enumerate()
            .map(|(i, &primitive)| BoardWorldVolume {
                body: CollisionBody::Attached(i),
                primitive,
                linear_velocity: Vector3::new(0., velocity, 0.),
                material: material(),
            })
            .collect();
        for (padding, maximum) in [(0., 0.), (0., 0.5), (0.2, 0.), (0.02, 0.5)] {
            let query = WorldContactSettings {
                volume_padding: padding,
                maximum_separating_distance: maximum,
                edge_cos_bend_normal_threshold: -1.,
                convexity_epsilon: 0.,
                is_object: false,
            };
            for capacity in [1, 3, 100] {
                for deferred_reduction in [false, true] {
                    let retention = ContactRetentionSettings {
                        capacity,
                        duplicate_distance_squared: 0.000001,
                        deferred_reduction,
                    };
                    let snapshot = |hits: &[BoardCollision]| {
                        hits.iter()
                            .map(|h| retention_record(h.body_a, h.contact))
                            .collect::<Vec<_>>()
                    };
                    let expected = snapshot(linear.query_primitives(&volumes, query, retention));
                    observed |= !expected.is_empty();
                    assert_eq!(
                        snapshot(world.query_primitives(&volumes, query, retention)),
                        expected
                    );
                    assert_eq!(world.dropped_contacts(), linear.dropped_contacts());
                }
            }
        }
    }
    assert!(observed);
}

fn water_face(height: f32) -> WorldTriangle {
    WorldTriangle::from_vertices(
        [
            Vector3::new(-2., height, -2.),
            Vector3::new(-2., height, 2.),
            Vector3::new(2., height, -2.),
        ],
        material(),
        0x637, // surface ID 1591: type 12
        0xe0,
        [1.; 3],
        0.,
    )
    .unwrap()
}

#[test]
fn water_is_not_solid_but_reports_its_surface() {
    assert!(is_water_tag(0x637) && !is_water_tag(0x637 - 0x80));
    let sphere = |y: f32| BoardWorldVolume {
        body: CollisionBody::Board(BodyId::Deck),
        primitive: ContactPrimitive::Sphere(Sphere {
            center: Vector3::new(-1., y, -1.),
            radius: 0.2,
        }),
        linear_velocity: Vector3::new(0., -1., 0.),
        material: material(),
    };
    let query = WorldContactSettings {
        volume_padding: 0.05,
        maximum_separating_distance: 0.1,
        edge_cos_bend_normal_threshold: -1.,
        convexity_epsilon: 0.,
        is_object: false,
    };
    let retention = ContactRetentionSettings {
        capacity: 100,
        duplicate_distance_squared: 0.000001,
        deferred_reduction: false,
    };
    // A sphere resting on a solid face at the same place touches it.
    let mut solid = BoardWorld::new(vec![face(0., 17, 0.)]);
    assert!(!solid.query_primitives(&[sphere(0.15)], query, retention).is_empty());
    // On water it produces no contact at all.
    let mut world = BoardWorld::new(vec![water_face(0.)]);
    assert!(world.query_primitives(&[sphere(0.15)], query, retention).is_empty());
    // Surface lookup: inside the triangle, below or just above the surface.
    assert_eq!(world.water_surface_at(Vector3::new(-1., -0.5, -1.), 0.05, 3.), Some(0.));
    assert_eq!(world.water_surface_at(Vector3::new(-1., 0.03, -1.), 0.05, 3.), Some(0.));
    assert_eq!(world.water_surface_at(Vector3::new(-1., 0.2, -1.), 0.05, 3.), None);
    assert_eq!(world.water_surface_at(Vector3::new(-1., -4., -1.), 0.05, 3.), None);
    assert_eq!(world.water_surface_at(Vector3::new(1.5, -0.5, 1.5), 0.05, 3.), None);
    // Rays still hit water (wipeout prediction and respawn checks rely on it).
    let hit = world
        .query_thin_line(Vector3::new(-1., 1., -1.), Vector3::new(-1., -1., -1.))
        .unwrap();
    assert!(hit.is_some_and(|h| is_water_tag(h.tag)));
}

#[test]
fn shallow_water_over_a_floor_is_not_deep_water() {
    // Water at y = 0 over a solid floor 5 cm below it (x < 0 half) only.
    let floor = WorldTriangle::from_vertices(
        [Vector3::new(-2., -0.05, -2.), Vector3::new(-2., -0.05, 2.), Vector3::new(0., -0.05, -2.)],
        material(),
        17,
        0xe0,
        [1.; 3],
        0.,
    )
    .unwrap();
    let world = BoardWorld::new(vec![water_face(0.), floor]);
    let over_floor = Vector3::new(-1.5, -0.02, -1.);
    let open = Vector3::new(0.5, -0.5, -1.5);
    assert_eq!(world.water_surface_at(over_floor, 0.05, 3.), Some(0.));
    assert_eq!(world.deep_water_surface_at(over_floor, 0.05, 3., 0.5), None);
    assert_eq!(world.deep_water_surface_at(open, 0.05, 3., 0.5), Some(0.));
}

#[test]
fn water_is_shallow_only_over_a_nearby_floor() {
    let floor = WorldTriangle::from_vertices(
        [Vector3::new(-3., -0.05, -3.), Vector3::new(-3., -0.05, 3.), Vector3::new(0., -0.05, -3.)],
        material(),
        17,
        0xe0,
        [1.; 3],
        0.,
    )
    .unwrap();
    let world = BoardWorld::new(vec![water_face(0.), floor]);
    assert!(world.water_shallow_at(Vector3::new(-1.5, 0., -1.5)));
    assert!(!world.water_shallow_at(Vector3::new(1., 0., -1.5)));
}

#[test]
fn shallow_water_is_solid_and_deep_water_is_not() {
    let floor = |y: f32| {
        WorldTriangle::from_vertices(
            [Vector3::new(-2., y, -2.), Vector3::new(-2., y, 2.), Vector3::new(2., y, -2.)],
            material(),
            17,
            0xe0,
            [1.; 3],
            0.,
        )
        .unwrap()
    };
    let sphere = |y: f32| BoardWorldVolume {
        body: CollisionBody::Board(BodyId::Deck),
        primitive: ContactPrimitive::Sphere(Sphere { center: Vector3::new(-1., y, -1.), radius: 0.2 }),
        linear_velocity: Vector3::new(0., -1., 0.),
        material: material(),
    };
    let query = WorldContactSettings {
        volume_padding: 0.05,
        maximum_separating_distance: 0.1,
        edge_cos_bend_normal_threshold: -1.,
        convexity_epsilon: 0.,
        is_object: false,
    };
    let retention = ContactRetentionSettings { capacity: 100, duplicate_distance_squared: 0.000001, deferred_reduction: false };
    // A channel 5 cm deep: the body rests on the water itself.
    let mut shallow = BoardWorld::new(vec![water_face(0.), floor(-0.05)]);
    let hits = shallow.query_primitives(&[sphere(0.15)], query, retention).to_vec();
    assert!(hits.iter().any(|h| is_water_tag(h.contact.tag)));
    // A pool 2 m deep: no contact with the water surface.
    let mut deep = BoardWorld::new(vec![water_face(0.), floor(-2.)]);
    assert!(deep.query_primitives(&[sphere(0.15)], query, retention).is_empty());
}
