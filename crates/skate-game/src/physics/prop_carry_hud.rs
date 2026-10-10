//! Minimal procedural indicator for prop carry/placement (Phase 4). No
//! authored assets: one small 2D diamond at the bottom-centre of the screen.
//! White: a grabbable prop is in reach while on foot. Cyan: carrying.
//! Yellow: placement mode. Hidden otherwise.
use bevy::{camera::visibility::RenderLayers, prelude::*};

const LAYER: usize = 30;

#[derive(Component)]
struct PropCarryIndicator;

pub(crate) fn install(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(Update, present);
}

fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let layer = RenderLayers::layer(LAYER);
    commands.spawn((
        Camera2d,
        Camera {
            order: 2,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Msaa::Off,
        layer.clone(),
    ));
    let diamond = [
        [0.0, 14.0, 0.0],
        [10.0, 0.0, 0.0],
        [0.0, -14.0, 0.0],
        [-10.0, 0.0, 0.0],
    ];
    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            diamond[0], diamond[1], diamond[2],
            diamond[0], diamond[2], diamond[3],
        ],
    );
    commands.spawn((
        PropCarryIndicator,
        Mesh2d(meshes.add(mesh)),
        MeshMaterial2d(materials.add(ColorMaterial::default())),
        Transform::from_xyz(0.0, -300.0, 0.0),
        Visibility::Hidden,
        layer,
    ));
}

fn present(
    physics: Res<super::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>,
    window: Single<&Window>,
    mut indicator: Query<
        (&mut Visibility, &MeshMaterial2d<ColorMaterial>, &mut Transform),
        With<PropCarryIndicator>,
    >,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let Ok((mut visibility, material, mut transform)) = indicator.single_mut() else {
        return;
    };
    let color = if physics.prop_carry.placing() {
        Some(Color::srgb(1.0, 0.85, 0.1))
    } else if physics.prop_carry.held().is_some() {
        Some(Color::srgb(0.2, 0.9, 1.0))
    } else {
        let root = skater.animated_skeleton.roots.animation_to_world;
        let flat = [root[2][0], root[2][2]];
        let length = (flat[0] * flat[0] + flat[1] * flat[1]).sqrt();
        let carrier = super::prop_carry::Carrier {
            state: skater.player_state.current(),
            position: skate_core::math::Vector3::new(root[3][0], root[3][1], root[3][2]),
            forward: if length > 1e-3 {
                skate_core::math::Vector3::new(flat[0] / length, 0.0, flat[1] / length)
            } else {
                skate_core::math::Vector3::new(0.0, 0.0, 1.0)
            },
            time_step: physics.period().as_secs_f32(),
            skeleton: None,
        };
        physics
            .prop_dynamics()
            .and_then(|d| physics.prop_carry.candidate(d, carrier))
            .map(|_| Color::srgb(1.0, 1.0, 1.0))
    };
    match color {
        Some(color) => {
            *visibility = Visibility::Inherited;
            if let Some(material) = materials.get_mut(&material.0) {
                material.color = color;
            }
            transform.translation =
                Vec3::new(0.0, -window.height() / 2.0 + 40.0, 0.0);
        }
        None => *visibility = Visibility::Hidden,
    }
}
