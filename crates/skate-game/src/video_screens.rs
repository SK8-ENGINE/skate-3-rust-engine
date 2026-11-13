//! Park videoscreens (jumbotrons): the skater's view without the HUD on every `incandescent.videoscreen`
//! material. docs/hails-additions/35-world-shading-parity.md#videoscreen-live-feed
use bevy::{
    asset::uuid_handle,
    camera::{
        primitives::{Aabb, Frustum},
        visibility::{CascadesVisibleEntities, VisibilitySystems, VisibleEntities},
        CameraUpdateSystems, RenderTarget,
    },
    core_pipeline::tonemapping::Tonemapping,
    image::ImageSampler,
    math::Affine3A,
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{CommandEncoderDescriptor, TextureFormat, TextureUsages},
        renderer::{RenderDevice, RenderQueue},
        texture::GpuImage,
        view::Hdr,
        Render, RenderApp, RenderSystems,
    },
    transform::TransformSystems,
};

use crate::camera::GameplayCamera;
use crate::retail_render::{FrameStateData, RetailTone, ToneOnly};

/// The copy every videoscreen material samples; the feed camera renders into [`Feed::target`].
pub(crate) const VIDEO_FEED: Handle<Image> = uuid_handle!("3c0f6a2e-58d4-4b7a-9e61-0d2b7f94c1a5");

/// Feed values. Not retail yet (resolution, rate unknown): mods change them through world tuning
/// `video_screens`.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct VideoScreenSettings {
    pub enabled: bool,
    pub size: UVec2,
    pub rate_hz: f32,
    pub max_distance: f32,
}

impl Default for VideoScreenSettings {
    fn default() -> Self {
        Self { enabled: true, size: UVec2::new(512, 288), rate_hz: 30.0, max_distance: 300.0 }
    }
}

/// One screen material of the loaded map, world-space bounds of its triangles.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct VideoScreen {
    /// `<map>/<material index>`, stable for a converted map.
    pub id: String,
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Component)]
pub(crate) struct FeedCamera;

#[derive(Resource)]
struct Feed {
    target: Handle<Image>,
    size: UVec2,
    clock: f32,
    live: bool,
    /// A feed frame renders this engine frame.
    due: bool,
}

#[derive(Resource, Clone, Default, ExtractResource)]
struct FeedCopy(Option<(AssetId<Image>, AssetId<Image>)>);

fn feed_image(size: UVec2, source: bool) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = ImageSampler::linear();
    if source {
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    }
    image
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, settings: Res<VideoScreenSettings>) {
    let size = settings.size;
    let _ = images.insert(VIDEO_FEED.id(), feed_image(size, false));
    let target = images.add(feed_image(size, true));
    commands.spawn((
        Name::new("video feed camera"),
        FeedCamera,
        ToneOnly,
        Camera3d::default(),
        Camera { is_active: false, order: -2, ..default() },
        RenderTarget::Image(target.clone().into()),
        Msaa::Off,
        Transform::default(),
    ));
    commands.insert_resource(Feed { target, size, clock: 0.0, live: false, due: false });
}

fn resize(settings: Res<VideoScreenSettings>, mut feed: ResMut<Feed>, mut images: ResMut<Assets<Image>>) {
    if feed.size == settings.size {
        return;
    }
    let size = settings.size;
    let _ = images.insert(VIDEO_FEED.id(), feed_image(size, false));
    let _ = images.insert(feed.target.id(), feed_image(size, true));
    feed.size = size;
    feed.live = false;
}

/// The gameplay camera's pose, lens and tone, before transforms propagate and projections update.
fn follow(
    mut commands: Commands,
    feed: Res<Feed>,
    gameplay: Query<(&Transform, &Projection, Has<Hdr>, Has<RetailTone>, Option<&Tonemapping>), (With<GameplayCamera>, Without<FeedCamera>)>,
    mut cameras: Query<(Entity, &mut Transform, &mut Projection, Has<Hdr>, Has<RetailTone>, Option<&Tonemapping>), With<FeedCamera>>,
) {
    let Some((pose, lens, hdr, tone, tonemapping)) = gameplay.iter().next() else { return };
    for (entity, mut transform, mut projection, has_hdr, has_tone, has_tonemapping) in &mut cameras {
        *transform = *pose;
        if let (Projection::Perspective(from), Projection::Perspective(to)) = (lens, &mut *projection) {
            let aspect = feed.size.x as f32 / feed.size.y as f32;
            if (to.fov, to.near, to.far, to.aspect_ratio) != (from.fov, from.near, from.far, aspect) {
                *to = PerspectiveProjection { aspect_ratio: aspect, ..from.clone() };
            }
        }
        let mut e = commands.entity(entity);
        match (hdr, has_hdr) {
            (true, false) => { e.insert(Hdr); }
            (false, true) => { e.remove::<Hdr>(); }
            _ => {}
        }
        match (tone, has_tone) {
            (true, false) => { e.insert(RetailTone); }
            (false, true) => { e.remove::<RetailTone>(); }
            _ => {}
        }
        if let Some(t) = tonemapping.filter(|t| has_tonemapping != Some(*t)) {
            e.insert(*t);
        }
    }
}

pub(crate) fn in_view(screen: &VideoScreen, frustum: &Frustum, eye: Vec3, max_distance: f32) -> bool {
    let aabb = Aabb::from_min_max(screen.min, screen.max);
    eye.clamp(screen.min, screen.max).distance(eye) <= max_distance
        && frustum.intersects_obb(&aabb, &Affine3A::IDENTITY, true, false)
}

/// Draws the feed on a frame-independent cadence while a screen is in the gameplay camera's view.
#[allow(clippy::too_many_arguments)]
fn schedule(
    time: Res<Time>,
    settings: Res<VideoScreenSettings>,
    mut feed: ResMut<Feed>,
    mut copy: ResMut<FeedCopy>,
    mut state: ResMut<FrameStateData>,
    screens: Query<&VideoScreen>,
    gameplay: Query<(&Camera, &GlobalTransform, &Frustum), (With<GameplayCamera>, Without<FeedCamera>)>,
    mut cameras: Query<&mut Camera, With<FeedCamera>>,
) {
    let wanted = settings.enabled
        && gameplay.iter().next().is_some_and(|(camera, at, frustum)| {
            camera.is_active && screens.iter().any(|s| in_view(s, frustum, at.translation(), settings.max_distance))
        });
    let period = 1.0 / settings.rate_hz;
    let render = wanted && tick(&mut feed.clock, time.delta_secs(), period);
    if !wanted {
        feed.clock = period;
    }
    let live = f32::from(u8::from(wanted && feed.live));
    if state.video_feed.x != live {
        info!("VIDEO_FEED {} size={} rate_hz={}", if live > 0.0 { "live" } else { "off" }, feed.size, settings.rate_hz);
        state.video_feed.x = live;
    }
    feed.live |= render;
    feed.due = render;
    copy.0 = render.then(|| (feed.target.id(), VIDEO_FEED.id()));
    // Active on every frame a screen is in view: a view skipped for 3 frames loses its cached depth, HDR and shadow
    // textures, and switching per feed frame re-created them each time. Frames between feed updates draw nothing
    // (`idle`).
    for mut camera in &mut cameras {
        if camera.is_active != wanted {
            camera.is_active = wanted;
        }
    }
}

/// Empties the feed view's draw lists (its own and its shadow cascades') on frames without a feed update.
fn idle(
    feed: Res<Feed>,
    mut cameras: Query<(Entity, &mut VisibleEntities), With<FeedCamera>>,
    mut lights: Query<&mut CascadesVisibleEntities>,
) {
    if feed.due {
        return;
    }
    for (entity, mut visible) in &mut cameras {
        visible.clear_all();
        for mut cascades in &mut lights {
            if let Some(views) = cascades.entities.get_mut(&entity) {
                for cascade in views {
                    cascade.entities.clear();
                }
            }
        }
    }
}

/// Advances the cadence clock; true when a feed frame is due (at most one per engine frame).
fn tick(clock: &mut f32, delta: f32, period: f32) -> bool {
    *clock += delta;
    if *clock < period {
        return false;
    }
    *clock = (*clock - period).min(period);
    true
}

fn copy_feed(copy: Res<FeedCopy>, images: Res<RenderAssets<GpuImage>>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let Some((from, to)) = copy.0 else { return };
    let (Some(from), Some(to)) = (images.get(from), images.get(to)) else { return };
    if from.texture.size() != to.texture.size() {
        return;
    }
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: Some("video feed copy") });
    encoder.copy_texture_to_texture(from.texture.as_image_copy(), to.texture.as_image_copy(), from.texture.size());
    queue.submit([encoder.finish()]);
}

pub(crate) struct VideoScreensPlugin;

impl Plugin for VideoScreensPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VideoScreenSettings>()
            .init_resource::<FeedCopy>()
            .add_plugins(ExtractResourcePlugin::<FeedCopy>::default())
            .add_systems(Startup, setup)
            .add_systems(
                PostUpdate,
                (
                    (resize, follow).chain().before(TransformSystems::Propagate).before(CameraUpdateSystems),
                    // Activation must precede the cascade build (bevy_pbr unwraps a cascade per active view);
                    // the gameplay frustum is then the previous frame's.
                    schedule
                        .after(TransformSystems::Propagate)
                        .before(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades)
                        .before(VisibilitySystems::CheckVisibility),
                    idle.after(VisibilitySystems::CheckVisibility)
                        .after(bevy::light::SimulationLightSystems::CheckLightVisibility),
                ),
            );
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.add_systems(Render, copy_feed.after(RenderSystems::Render).before(RenderSystems::Cleanup));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::CameraProjection;

    #[test]
    fn cadence_gives_the_same_updates_per_second_at_any_frame_rate() {
        for fps in [30.0f32, 60.0, 144.0, 240.0] {
            let mut clock = 1.0 / 30.0;
            let frames = (fps * 10.0) as usize;
            let updates = (0..frames).filter(|_| tick(&mut clock, 1.0 / fps, 1.0 / 30.0)).count();
            assert!((299..=301).contains(&updates), "{fps} fps: {updates} updates in 10 s");
        }
        // Slower than the rate: every frame updates, never two per frame.
        let mut clock = 0.0;
        assert_eq!((0..200).filter(|_| tick(&mut clock, 1.0 / 20.0, 1.0 / 30.0)).count(), 200);
    }

    #[test]
    fn screens_count_only_in_front_and_within_distance() {
        let camera = Transform::from_xyz(0.0, 0.0, 0.0).looking_at(Vec3::NEG_Z, Vec3::Y);
        let projection = PerspectiveProjection::default();
        let clip = projection.get_clip_from_view();
        let frustum = Frustum::from_clip_from_world_custom_far(
            &(clip * camera.to_matrix().inverse()),
            &camera.translation,
            &camera.back(),
            projection.far,
        );
        let screen = |z: f32| VideoScreen { id: "t/0".into(), min: Vec3::new(-2.0, -1.0, z), max: Vec3::new(2.0, 1.0, z + 0.1) };
        assert!(in_view(&screen(-20.0), &frustum, camera.translation, 300.0));
        assert!(!in_view(&screen(20.0), &frustum, camera.translation, 300.0), "behind the camera");
        assert!(!in_view(&screen(-400.0), &frustum, camera.translation, 300.0), "beyond the distance");
    }
}
