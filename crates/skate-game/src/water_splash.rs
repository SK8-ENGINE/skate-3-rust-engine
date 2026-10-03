//! Water entry splash. Retail footage (PS3 version in RPCS3, Aletown canal):
//! when the skater falls into deep water a white spray plume bursts up around
//! the body, about 3-4 m across, and is gone after ~0.7 s; a smaller puff
//! appears where the board hits the water. Shallow water (the University
//! fountain channel) shows none. Retail builds the plume in code from its
//! `water` particle sprite; setup extracts that sprite from the owned disc
//! (tools/asset_pipeline/particles.py -> assets/private/particles/water.png).
//! This is an own, minimal re-implementation: camera-facing sprites with
//! random rolls, rising, slowing, growing and fading. Counts, sizes, speeds and
//! timings are matched by eye (docs/hails-additions/09-water.md). Without the
//! sprite there is no splash.
use crate::physics::GamePhysics;
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
};
use skate_core::{
    math::Vector3,
    physics::{board::BodyId, board_world::FLOAT_DEPTH},
};

/// A body counts as entering water up to this far above the surface.
const ABOVE: f32 = 0.05;
const MAX_DEPTH: f32 = 4.0;
/// Downward speed (m/s) below which an entry makes no splash.
const MIN_SPEED: f32 = 1.0;
/// Sprite brightness: retail world materials output with baseline exposure
/// 2.5 before the tone pass, so the sprite needs similar headroom.
const BRIGHTNESS: f32 = 1.9;

pub(crate) struct WaterSplashPlugin;

impl Plugin for WaterSplashPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, (detect, animate).chain());
    }
}

#[derive(Resource)]
struct SplashAssets {
    quad: Handle<Mesh>,
    texture: Handle<Image>,
}

#[derive(Component)]
struct Particle {
    velocity: Vec3,
    age: f32,
    life: f32,
    size: (f32, f32),
    alpha: f32,
    gravity: f32,
    drag: f32,
    roll: f32,
    spin: f32,
}

fn setup(
    mut commands: Commands,
    config: Res<crate::config::Config>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
) {
    let path = config.asset_root.join("private/particles/water.png");
    let image = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| {
        Image::from_buffer(
            &bytes,
            ImageType::MimeType("image/png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::default(),
            RenderAssetUsages::RENDER_WORLD,
        )
        .map_err(|e| e.to_string())
    });
    match image {
        Ok(image) => {
            let texture = images.add(image);
            let quad = meshes.add(Rectangle::new(1.0, 1.0));
            commands.insert_resource(SplashAssets { quad, texture });
        }
        Err(error) => info!("Water splash disabled ({}): {error}", path.display()),
    }
}

#[derive(Default)]
struct EntryState {
    skater: bool,
    board: bool,
    rng: u32,
}

fn deep_surface(physics: &GamePhysics, p: Vector3) -> Option<f32> {
    physics.world().deep_water_surface_at(p, ABOVE, MAX_DEPTH, FLOAT_DEPTH)
}

/// Splashes on the frame the skater root or the board deck enters deep water.
fn detect(
    mut commands: Commands,
    physics: Option<Res<GamePhysics>>,
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    assets: Option<Res<SplashAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: Local<EntryState>,
) {
    let (Some(physics), Some(skater), Some(assets)) = (physics, skater, assets) else { return };
    if state.rng == 0 {
        state.rng = 0x9e37_79b9;
    }
    let root = skater.skeleton.bodies()[0].rates;
    let deck = physics.board.bodies()[BodyId::Deck.index()].rates;
    for (is_board, body) in [(false, root), (true, deck)] {
        let surface = deep_surface(&physics, body.position);
        let was = if is_board { state.board } else { state.skater };
        let speed = -body.linear_velocity.y;
        if let Some(surface) = surface
            && !was
            && speed > MIN_SPEED
        {
            let at = Vec3::new(body.position.x, surface, body.position.z);
            let strength = (speed / 8.0).clamp(0.4, 1.2) * if is_board { 0.45 } else { 1.0 };
            spawn_plume(&mut commands, &assets, &mut materials, &mut state.rng, at, strength);
        }
        if is_board {
            state.board = surface.is_some();
        } else {
            state.skater = surface.is_some();
        }
    }
}

fn random(rng: &mut u32) -> f32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    (*rng >> 8) as f32 / (1u32 << 24) as f32
}

fn range(rng: &mut u32, (low, high): (f32, f32)) -> f32 {
    low + (high - low) * random(rng)
}

/// A spray plume of water sprites; `strength` scales count, size and speed.
fn spawn_plume(
    commands: &mut Commands,
    assets: &SplashAssets,
    materials: &mut Assets<StandardMaterial>,
    rng: &mut u32,
    at: Vec3,
    strength: f32,
) {
    let count = (13.0 * strength).round().max(3.0) as usize;
    let scale = strength.sqrt();
    for _ in 0..count {
        let angle = random(rng) * std::f32::consts::TAU;
        let direction = Vec3::new(angle.cos(), 0.0, angle.sin());
        let radius = range(rng, (0.0, 0.6)) * scale;
        let size = (range(rng, (0.8, 1.3)) * scale, range(rng, (1.8, 2.8)) * scale);
        let velocity = direction * range(rng, (0.4, 1.6)) * scale + Vec3::Y * range(rng, (1.6, 3.8)) * scale;
        let alpha = range(rng, (0.45, 0.72));
        let material = materials.add(StandardMaterial {
            base_color: Color::linear_rgba(BRIGHTNESS, BRIGHTNESS, BRIGHTNESS, 0.0),
            base_color_texture: Some(assets.texture.clone()),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(at + direction * radius + Vec3::Y * (0.1 + 0.3 * random(rng)))
                .with_scale(Vec3::splat(size.0)),
            Particle {
                velocity,
                age: 0.0,
                life: range(rng, (0.55, 0.85)),
                size,
                alpha,
                gravity: 5.0,
                drag: 2.2,
                roll: random(rng) * std::f32::consts::TAU,
                spin: range(rng, (-1.0, 1.0)),
            },
        ));
    }
}

/// Moves, grows, fades and billboards particles; removes them when done.
fn animate(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<&GlobalTransform, With<crate::camera::GameplayCamera>>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs().min(0.05);
    let facing = camera.iter().next().map(|c| c.compute_transform().rotation);
    for (entity, mut particle, mut transform, material) in &mut particles {
        particle.age += dt;
        if particle.age >= particle.life {
            materials.remove(&material.0);
            commands.entity(entity).despawn();
            continue;
        }
        let t = particle.age / particle.life;
        let drag = (1.0 - particle.drag * dt).max(0.0);
        particle.velocity *= drag;
        particle.velocity.y -= particle.gravity * dt;
        let step = particle.velocity * dt;
        transform.translation += step;
        // Grow quickly at first, then slowly.
        let grow = 1.0 - (1.0 - t).powi(2);
        transform.scale = Vec3::splat(particle.size.0 + (particle.size.1 - particle.size.0) * grow);
        particle.roll += particle.spin * dt;
        if let Some(rotation) = facing {
            transform.rotation = rotation * Quat::from_rotation_z(particle.roll);
        }
        if let Some(m) = materials.get_mut(&material.0) {
            // Quick fade in, then out.
            let fade = (t / 0.06).min(1.0) * (1.0 - t).powf(1.3);
            m.base_color.set_alpha(particle.alpha * fade);
        }
    }
}
