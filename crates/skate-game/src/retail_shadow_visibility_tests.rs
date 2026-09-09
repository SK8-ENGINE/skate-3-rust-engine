use super::*;
fn frustum(radius: f32, x: f32) -> Frustum {
    Frustum::from_clip_from_world(
        &(Mat4::orthographic_rh(-radius, radius, -radius, radius, 0.1, 300.)
            * Mat4::look_at_rh(Vec3::new(x, 20., 80.), Vec3::new(x, 0., 0.), Vec3::Y)),
    )
}
#[test]
fn spatial_tree_matches_flat_obb_checks_with_rotations_and_layers() {
    let mut world = World::new();
    let mut entries = vec![];
    for i in 0..4096 {
        entries.push(Entry {
            entity: world.spawn_empty().id(),
            aabb: Aabb {
                center: bevy::math::Vec3A::ZERO,
                half_extents: bevy::math::Vec3A::new(1., 2., 3.),
            },
            transform: GlobalTransform::from(
                Transform::from_xyz((i % 64) as f32 * 8. - 256., 0., (i / 64) as f32 * 8. - 256.)
                    .with_rotation(Quat::from_rotation_y(i as f32 * 0.17))
                    .with_scale(Vec3::new(1., 2., 0.5)),
            ),
            visible: i % 9 != 0,
            layers: RenderLayers::layer(i % 2),
        });
    }
    let mut index = Index::default();
    index.rebuild(entries);
    for x in [-200., -30., 0., 45., 180.] {
        for layer in [0, 1] {
            for radius in [5., 40., 150.] {
                let f = frustum(radius, x);
                let mask = RenderLayers::layer(layer);
                let mut expected: Vec<_> = index
                    .entries
                    .iter()
                    .filter(|e| {
                        e.visible
                            && e.layers.intersects(&mask)
                            && f.intersects_obb(&e.aabb, &e.transform.affine(), false, true)
                    })
                    .map(|e| e.entity)
                    .collect();
                let mut actual = vec![];
                index.visit(0, &f, &mask, &mut actual);
                expected.sort();
                actual.sort();
                assert_eq!(actual, expected);
            }
        }
    }
}
#[test]
fn indexed_system_matches_upstream_across_changes_and_removal() {
    bevy::tasks::ComputeTaskPool::get_or_init(Default::default);
    let mut world = World::new();
    world.init_resource::<Index>();
    let view = world.spawn_empty().id();
    let visibility = ViewVisibility::HIDDEN;
    let mut frusta = CascadesFrusta::default();
    frusta
        .frusta
        .insert(view, vec![frustum(10., 0.), frustum(100., 0.)]);
    let light = world
        .spawn((
            DirectionalLight {
                shadows_enabled: true,
                ..default()
            },
            frusta,
            CascadesVisibleEntities::default(),
            visibility,
        ))
        .id();
    world
        .get_mut::<ViewVisibility>(light)
        .unwrap()
        .set_visible();
    let mut entities = vec![];
    for i in 0..80 {
        let mut e = world.spawn((
            Mesh3d::default(),
            Aabb {
                center: bevy::math::Vec3A::ZERO,
                half_extents: bevy::math::Vec3A::ONE,
            },
            GlobalTransform::from_translation(Vec3::new(i as f32 * 5. - 200., 0., 0.)),
            InheritedVisibility::VISIBLE,
        ));
        if i % 2 == 0 {
            e.insert(StaticShadowCaster);
        }
        if i % 7 == 0 {
            e.insert(NoFrustumCulling);
        }
        if i % 11 == 0 {
            e.insert(RenderLayers::layer(28));
        }
        entities.push(e.id());
    }
    let mut original = bevy::ecs::schedule::Schedule::default();
    original.add_systems(bevy::light::check_dir_light_mesh_visibility);
    let mut indexed = bevy::ecs::schedule::Schedule::default();
    indexed.add_systems((refresh, check_indexed).chain());
    let read = |world: &World| {
        let v = world.get::<CascadesVisibleEntities>(light).unwrap();
        let mut lists: Vec<Vec<Entity>> = v.entities[&view]
            .iter()
            .map(|e| e.entities.clone())
            .collect();
        for list in &mut lists {
            list.sort();
        }
        lists
    };
    for step in 0..6 {
        match step {
            1 => {
                world
                    .entity_mut(entities[0])
                    .insert(GlobalTransform::from_translation(Vec3::ZERO));
            }
            2 => {
                world.entity_mut(entities[2]).insert(NotShadowCaster);
                world
                    .entity_mut(entities[4])
                    .insert(InheritedVisibility::HIDDEN);
            }
            3 => {
                world.entity_mut(entities[2]).remove::<NotShadowCaster>();
                world.entity_mut(entities[11]).remove::<RenderLayers>();
            }
            4 => {
                world
                    .entity_mut(light)
                    .get_mut::<DirectionalLight>()
                    .unwrap()
                    .shadows_enabled = false;
            }
            5 => {
                world
                    .entity_mut(light)
                    .get_mut::<DirectionalLight>()
                    .unwrap()
                    .shadows_enabled = true;
                world.despawn(entities[6]);
            }
            _ => {}
        }
        original.run(&mut world);
        let expected = read(&world);
        indexed.run(&mut world);
        assert_eq!(read(&world), expected, "step {step}");
    }
    for entity in entities {
        world.despawn(entity);
    }
    indexed.run(&mut world);
    assert!(world.resource::<Index>().entries.is_empty());
}
#[test]
fn replacement_preserves_schedule_ordering() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::transform::TransformPlugin,
        bevy::mesh::MeshPlugin,
        bevy::camera::visibility::VisibilityPlugin,
        bevy::light::LightPlugin,
    ));
    install(&mut app);
    finish(&mut app);
}

pub(super) fn benchmark_bounds(bounds: Vec<Aabb>) -> serde_json::Value {
    let mut world = World::new();
    let mut index = Index::default();
    let count = bounds.len();
    let start = std::time::Instant::now();
    index.rebuild(
        bounds
            .into_iter()
            .map(|aabb| Entry {
                entity: world.spawn_empty().id(),
                aabb,
                transform: GlobalTransform::IDENTITY,
                visible: true,
                layers: default(),
            })
            .collect(),
    );
    let build_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut frusta = vec![];
    for offset in [-50., 0., 50.] {
        for radius in [5., 10., 30., 100.] {
            let center = Vec3::new(330. + offset, 133., -710.);
            let light = Vec3::new(-0.60385966, 0.73098797, 0.31782088);
            frusta.push(Frustum::from_clip_from_world(
                &(Mat4::orthographic_rh(-radius, radius, -radius, radius, 0.1, 400.)
                    * Mat4::look_at_rh(center + light * 200., center, Vec3::Y)),
            ));
        }
    }
    let mask = RenderLayers::default();
    let mut out = Vec::with_capacity(count);
    let mut expected = Vec::with_capacity(count);
    for f in &frusta {
        out.clear();
        expected.clear();
        index.visit(0, f, &mask, &mut out);
        for entry in &index.entries {
            if f.intersects_obb(&entry.aabb, &entry.transform.affine(), false, true) {
                expected.push(entry.entity);
            }
        }
        out.sort();
        expected.sort();
        assert_eq!(out, expected);
    }
    let repetitions = 100;
    let start = std::time::Instant::now();
    for _ in 0..repetitions {
        for f in &frusta {
            out.clear();
            for entry in &index.entries {
                if std::hint::black_box(f).intersects_obb(
                    &entry.aabb,
                    &entry.transform.affine(),
                    false,
                    true,
                ) {
                    out.push(entry.entity);
                }
            }
            std::hint::black_box(&out);
        }
    }
    let flat_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = std::time::Instant::now();
    for _ in 0..repetitions {
        for f in &frusta {
            out.clear();
            index.visit(0, std::hint::black_box(f), &mask, &mut out);
            std::hint::black_box(&out);
        }
    }
    let indexed_ms = start.elapsed().as_secs_f64() * 1000.;
    serde_json::json!({"static_batches":count,"queries":repetitions*frusta.len(),"flat_ms":flat_ms,"indexed_ms":indexed_ms,"build_ms":build_ms,"visible_sets_equal":true,"note":"real University material bounds; representative shadow frusta, single-thread query kernel only, not whole-frame FPS"})
}
