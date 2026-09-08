use crate::assets::AssetManifest;
use bevy::prelude::*;

/// World transform owner. Animation only writes descendant bone transforms.
#[derive(Component)]
pub(crate) struct PlayerRoot;

pub(crate) struct WorldPlugin;
impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.065, 0.08, 0.10)))
            .insert_resource(GlobalAmbientLight {
                color: Color::WHITE,
                brightness: 350.,
                ..default()
            })
            .add_systems(Startup, spawn);
    }
}
fn spawn(
    mut commands: Commands,
    server: Res<AssetServer>,
    manifest: Res<AssetManifest>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut retail_materials: ResMut<Assets<crate::retail_render::RetailWorldMaterial>>,
    mut sky_materials: ResMut<Assets<crate::retail_render::RetailSkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut config: ResMut<crate::config::Config>,
) {
    commands
        .spawn((PlayerRoot, Transform::default(), Visibility::default()))
        .with_children(|parent| {
            parent.spawn(SceneRoot(server.load(
                GltfAssetLabel::Scene(0).from_asset(manifest.0.character_scene.clone()),
            )));
        });
    if let Some(map) = config.map.take() {
        // Physics has already consumed the package. Render assets own their
        // uploaded data; keeping another full city package wastes gigabytes.
        crate::skate_world::spawn(&map, &mut commands, &mut meshes, &mut materials, &mut retail_materials, &mut images, &crate::retail_render::MaterialTuning::load(&config.asset_root));
        crate::retail_render::spawn_backdrop(&map.name, &config.asset_root, &mut commands, &mut meshes, &mut materials, &mut retail_materials, &mut images);
        crate::retail_render::spawn_sky(&map.name, &config.asset_root, &mut commands, &mut meshes, &mut images, &mut sky_materials, &mut retail_materials);
        return;
    }
    let colors = [
        Color::srgb(0.16, 0.19, 0.21),
        Color::srgb(0.48, 0.35, 0.22),
        Color::srgb(0.30, 0.43, 0.48),
        Color::srgb(0.24, 0.48, 0.31),
    ];
    for (quads, color) in crate::physics::ground::surfaces().into_iter().zip(colors) {
        let positions: Vec<[f32; 3]> = quads.into_iter().flat_map(|vertices| {
            [0, 2, 1, 0, 3, 2].map(|i| {
                let v = vertices[i];
                [v.x, v.y, v.z]
            })
        }).collect();
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        ).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.compute_flat_normals();
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.9,
                double_sided: true,
                cull_mode: None,
                ..default()
            })),
            Transform::default(),
        ));
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 11000.,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4., 7., 4.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
