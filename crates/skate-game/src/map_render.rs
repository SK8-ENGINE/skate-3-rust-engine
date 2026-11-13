//! Off-thread scene preparation and transactional publish.
//!
//! Assets are built on a worker with *reserved* handles, then inserted on the
//! main thread in one pass. Nothing touches live ECS until commit, so a failed
//! load cannot leave a half-replaced world (RFC 1 §10.2).
//!
//! Retirement is the leak boundary: `MapAssets` owns every asset the scene
//! created, and dropping a map releases exactly those. Unlike the previous
//! design this does not hardcode the list of asset types — each staged group
//! carries its own release behaviour, so adding a renderer asset type needs no
//! change here.
use bevy::{
    asset::{Asset, AssetId, Assets, AssetHandleProvider},
    ecs::world::CommandQueue,
    prelude::*,
};
use skate_data::skate_map::SkateMap;
use std::path::Path;

/// Marks everything the current map owns. Retirement despawns exactly this set,
/// which is why the character and other persistent entities survive a map change.
#[derive(Component)]
pub(crate) struct MapEntity;

/// Somewhere to put an asset that may not be the live `Assets<A>` collection.
pub(crate) trait AssetSink<A: Asset> {
    fn add(&mut self, asset: A) -> Handle<A>;
}

/// Erased release behaviour for one staged asset type.
trait OwnedAssets: Send + Sync {
    fn release(&mut self, world: &mut World);
}

/// Assets built off-thread against reserved handles.
///
/// `reserve_handle` hands out a real, strong handle without needing `&mut
/// Assets<A>`, which is what makes worker-thread construction possible at all.
pub(crate) struct StagedAssets<A: Asset> {
    provider: AssetHandleProvider,
    pending: Vec<(Handle<A>, A)>,
    owned: Vec<AssetId<A>>,
}

impl<A: Asset> StagedAssets<A> {
    pub(crate) fn new(world: &World) -> Self {
        Self {
            provider: world.resource::<Assets<A>>().get_handle_provider(),
            pending: Vec::new(),
            owned: Vec::new(),
        }
    }

    pub(crate) fn publish(&mut self, world: &mut World) {
        self.publish_some(&mut world.resource_mut::<Assets<A>>(), usize::MAX, |_| 0);
    }

    /// Insert staged assets in staging order until `budget` bytes (`size` of each) went in, at
    /// least one; returns the bytes inserted.
    fn publish_some(&mut self, assets: &mut Assets<A>, budget: usize, size: impl Fn(&A) -> usize) -> usize {
        let mut bytes = 0usize;
        let mut taken = 0;
        for (_, asset) in &self.pending {
            if taken > 0 && bytes >= budget {
                break;
            }
            bytes = bytes.saturating_add(size(asset));
            taken += 1;
        }
        for (handle, asset) in self.pending.drain(..taken) {
            self.owned.push(handle.id());
            // The handle came from this same provider moments ago, so a stale
            // generation would mean the reservation logic is broken, not that a
            // map failed to load. Say so rather than dropping geometry silently.
            if let Err(error) = assets.insert(handle.id(), asset) {
                error!(
                    "staged {} asset rejected on publish: {error}",
                    std::any::type_name::<A>()
                );
            }
        }
        bytes
    }
}

impl<A: Asset> AssetSink<A> for StagedAssets<A> {
    fn add(&mut self, asset: A) -> Handle<A> {
        let handle = self.provider.reserve_handle().typed::<A>();
        self.pending.push((handle.clone(), asset));
        handle
    }
}

impl<A: Asset> OwnedAssets for StagedAssets<A> {
    fn release(&mut self, world: &mut World) {
        let mut assets = world.resource_mut::<Assets<A>>();
        for id in self.owned.drain(..) {
            assets.remove(id);
        }
        self.pending.clear();
    }
}

/// Deferred spawns. Every entity created through this gets `MapEntity`, so
/// retirement cannot miss one by omission.
#[derive(Default)]
pub(crate) struct SceneCommands {
    queue: CommandQueue,
}

impl SceneCommands {
    pub(crate) fn spawn<B: Bundle>(&mut self, bundle: B) {
        self.queue.push(move |world: &mut World| {
            world.spawn((bundle, MapEntity));
        });
    }

    /// Apply the queued spawns now (tests; production goes through
    /// `PreparedScene::publish`).
    #[cfg(test)]
    pub(crate) fn apply(&mut self, world: &mut World) {
        std::mem::take(&mut self.queue).apply(world);
    }

    /// A parent with children in one deferred command. Only the parent gets
    /// `MapEntity`: despawning it removes the children with it, and marking
    /// them too would make retirement despawn them twice.
    pub(crate) fn spawn_with_children<B: Bundle, C: Bundle>(&mut self, bundle: B, children: Vec<C>) {
        self.queue.push(move |world: &mut World| {
            let parent = world.spawn((bundle, MapEntity)).id();
            for child in children {
                world.spawn((child, ChildOf(parent)));
            }
        });
    }
}

/// Assets owned by the live scene, released on the next map change.
#[derive(Resource, Default)]
pub(crate) struct MapAssets {
    groups: Vec<Box<dyn OwnedAssets>>,
    /// A map switch's meshes and textures, inserted a slice per frame after the commit ([`upload_slices`]).
    uploads: Option<(StagedAssets<Mesh>, StagedAssets<Image>)>,
}

impl MapAssets {
    /// Despawn the current map and release its assets. Safe to call with no map
    /// loaded, which is the case on the very first transition.
    pub(crate) fn retire(world: &mut World) {
        let entities: Vec<Entity> = world
            .query_filtered::<Entity, With<MapEntity>>()
            .iter(world)
            .collect();
        for entity in entities {
            world.despawn(entity);
        }
        if let Some(mut assets) = world.remove_resource::<MapAssets>() {
            for group in &mut assets.groups {
                group.release(world);
            }
            if let Some((meshes, images)) = assets.uploads.as_mut() {
                meshes.release(world);
                images.release(world);
            }
        }
    }

    /// Meshes or textures of the current map are still to be inserted.
    pub(crate) fn uploading(&self) -> bool {
        self.uploads.is_some()
    }
}

/// A whole scene, built without touching the live world.
pub(crate) struct PreparedScene {
    meshes: StagedAssets<Mesh>,
    images: StagedAssets<Image>,
    standard: StagedAssets<StandardMaterial>,
    world_materials: StagedAssets<crate::retail_render::WorldMaterial>,
    sky: StagedAssets<crate::retail_sky::SkyMaterial>,
    params: StagedAssets<bevy::render::storage::ShaderStorageBuffer>,
    commands: SceneCommands,
    pub stats: crate::skate_world::SceneStats,
    pub retail: bool,
}

impl PreparedScene {
    pub(crate) fn new(world: &World) -> Self {
        Self {
            meshes: StagedAssets::new(world),
            images: StagedAssets::new(world),
            standard: StagedAssets::new(world),
            world_materials: StagedAssets::new(world),
            sky: StagedAssets::new(world),
            params: StagedAssets::new(world),
            commands: SceneCommands::default(),
            stats: default(),
            retail: false,
        }
    }

    /// Worker-thread body. Builds all geometry, materials and images for the map.
    pub(crate) fn prepare(&mut self, map: Option<&SkateMap>, asset_root: &Path) {
        let Some(map) = map else {
            crate::world::spawn_test_world(&mut self.commands, &mut self.meshes, &mut self.standard);
            return;
        };
        self.retail = crate::retail_render::RetailScene::for_map(map);
        let tuning = crate::retail_render::MaterialTuning::load(asset_root);
        // The sky is loaded before the world because its authored fog frame and
        // sun direction are encoded into the per-material storage buffer while
        // the material table is built, not patched in afterwards.
        let sky = self.retail.then(|| {
            crate::retail_sky::SkyPackage::load(asset_root, &map.name)
                .inspect_err(|error| warn!("Retail sky unavailable for {}: {error}", map.name))
                .ok()
        }).flatten();
        let environment = sky
            .as_ref()
            .map(crate::retail_sky::SkyPackage::environment)
            .unwrap_or_default();
        self.stats = crate::skate_world::spawn(
            map,
            &tuning,
            &environment,
            &mut self.commands,
            &mut self.meshes,
            &mut self.world_materials,
            &mut self.images,
            &mut self.params,
        );
        // The district's global presentation model (ocean, far sea planes,
        // tree walls): retail districts only. Dropped by the renderer rewrite
        // (upstream e2b85b64) together with the props; restored here.
        if self.retail {
            if let Some(backdrop) = crate::retail_backdrop::load_package(asset_root, &map.name) {
                crate::skate_world::spawn_backdrop(
                    &backdrop,
                    &tuning,
                    &environment,
                    &mut self.commands,
                    &mut self.meshes,
                    &mut self.world_materials,
                    &mut self.images,
                    &mut self.params,
                );
            }
            // Unpaired far-proxy cells (Industrial's south hills under the
            // tree wall), as retail's per-cell proxy swap leaves them.
            if let Some(proxy) = crate::retail_backdrop::load_proxy_package(asset_root, &map.name) {
                crate::skate_world::spawn_proxy_terrain(
                    &proxy,
                    &tuning,
                    &environment,
                    &mut self.commands,
                    &mut self.meshes,
                    &mut self.world_materials,
                    &mut self.images,
                    &mut self.params,
                );
            }
        }
        // Movable props (SK8-ENGINE PR #15): retail districts only, as before
        // the renderer rewrite.
        if self.retail {
            if let Some((props, objects)) =
                crate::skate_world::load_map_prop_package(asset_root, map)
            {
                crate::skate_world::spawn_instances(
                    &props,
                    &objects,
                    &tuning,
                    &environment,
                    &mut self.commands,
                    &mut self.meshes,
                    &mut self.world_materials,
                    &mut self.images,
                    &mut self.params,
                    None,
                );
            }
            // The skate.Park layout the map was loaded with (park_layout.rs): its placed pieces, fixed, each drawn with
            // its tint and its world's shadow colours (the environmentpark families, doc 39 "Park shading").
            if let Some((pieces, objects)) = crate::park_layout::load_package(asset_root, map) {
                let draws = crate::park_layout::load_draws(asset_root, map);
                crate::skate_world::spawn_instances(
                    &pieces,
                    &objects,
                    &tuning,
                    &environment,
                    &mut self.commands,
                    &mut self.meshes,
                    &mut self.world_materials,
                    &mut self.images,
                    &mut self.params,
                    Some(&draws),
                );
            }
        }
        if let Some(sky) = sky {
            sky.spawn(
                &mut self.commands,
                &mut self.meshes,
                &mut self.images,
                &mut self.sky,
            );
        }
    }

    /// Main-thread commit. Assets first, then entities: a spawned entity must
    /// never reference a handle whose asset has not been inserted yet.
    pub(crate) fn publish(&mut self, world: &mut World) {
        self.meshes.publish(world);
        self.images.publish(world);
        self.publish_rest(world);
    }

    /// The map switch's commit: as [`PreparedScene::publish`], but the meshes and textures go in
    /// slices over the next frames ([`upload_slices`]) so the render thread does not upload a
    /// district's 1.1 GB in one frame. Their entities spawn now and draw once their assets are in;
    /// the transition holds the paused world until then (doc 25, "Map switch: the render upload").
    pub(crate) fn publish_sliced(&mut self, world: &mut World) {
        if slice_bytes() == usize::MAX {
            self.publish(world);
            return;
        }
        let meshes = std::mem::replace(&mut self.meshes, StagedAssets::new(world));
        let images = std::mem::replace(&mut self.images, StagedAssets::new(world));
        self.publish_rest(world);
        world.resource_mut::<MapAssets>().uploads = Some((meshes, images));
    }

    fn publish_rest(&mut self, world: &mut World) {
        self.standard.publish(world);
        self.world_materials.publish(world);
        self.sky.publish(world);
        self.params.publish(world);
        std::mem::take(&mut self.commands.queue).apply(world);
        world.insert_resource(MapAssets {
            groups: vec![
                Box::new(std::mem::replace(&mut self.meshes, StagedAssets::new(world))),
                Box::new(std::mem::replace(&mut self.images, StagedAssets::new(world))),
                Box::new(std::mem::replace(&mut self.standard, StagedAssets::new(world))),
                Box::new(std::mem::replace(&mut self.world_materials, StagedAssets::new(world))),
                Box::new(std::mem::replace(&mut self.sky, StagedAssets::new(world))),
                Box::new(std::mem::replace(&mut self.params, StagedAssets::new(world))),
            ],
            uploads: None,
        });
    }
}

/// Bytes of meshes and textures a map switch inserts per frame (`SKATE_MAP_UPLOAD_MB`, default 64; 0 = all at once,
/// as before). At least one asset goes in each frame.
fn slice_bytes() -> usize {
    static BYTES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BYTES.get_or_init(|| match std::env::var("SKATE_MAP_UPLOAD_MB").ok().and_then(|v| v.trim().parse::<usize>().ok()) {
        Some(0) => usize::MAX,
        Some(mb) => mb << 20,
        None => 64 << 20,
    })
}

fn mesh_bytes(mesh: &Mesh) -> usize {
    mesh.get_vertex_size() as usize * mesh.count_vertices() + mesh.indices().map_or(0, |i| i.len() * 4)
}

fn image_bytes(image: &Image) -> usize {
    image.data.as_ref().map_or(0, Vec::len)
}

/// A slice of the pending map meshes, then textures, per frame; the rest of the frame's budget carries over.
pub(crate) fn upload_slices(map: Option<ResMut<MapAssets>>, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>) {
    let Some(mut map) = map else { return };
    let map = &mut *map;
    let Some((staged_meshes, staged_images)) = map.uploads.as_mut() else { return };
    let mut budget = slice_bytes();
    if !staged_meshes.pending.is_empty() {
        budget = budget.saturating_sub(staged_meshes.publish_some(&mut meshes, budget, mesh_bytes));
    }
    if staged_meshes.pending.is_empty() && budget > 0 && !staged_images.pending.is_empty() {
        staged_images.publish_some(&mut images, budget, image_bytes);
    }
    if staged_meshes.pending.is_empty() && staged_images.pending.is_empty() {
        let (meshes, images) = map.uploads.take().expect("checked above");
        map.groups.push(Box::new(meshes));
        map.groups.push(Box::new(images));
        info!("MAP_UPLOAD done");
    }
}

// ---------------------------------------------------------------------------
// Day/night for non-retail maps
//
// Retail maps are lightmap-baked and keep their authored lighting, so these only
// drive the procedural test/custom world.
// ---------------------------------------------------------------------------

#[derive(Component)]
pub(crate) struct DayEnvironment {
    pub hour: f32,
    pub speed: f32,
}

/// Sun or moon disc, positioned opposite each other on the same arc.
#[derive(Component)]
pub(crate) struct CelestialBody {
    pub moon: bool,
}

pub(crate) fn advance_day(
    time: Res<Time<Virtual>>,
    mut lights: Query<(&mut DayEnvironment, &mut DirectionalLight, &mut Transform)>,
) {
    for (mut environment, mut light, mut transform) in &mut lights {
        environment.hour =
            (environment.hour + time.delta_secs() * environment.speed / 3600.0).rem_euclid(24.0);
        let angle = (environment.hour / 24.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let direction = Vec3::new(angle.cos(), angle.sin(), 0.25).normalize();
        *transform = Transform::from_translation(direction * 100.0)
            .looking_at(Vec3::ZERO, Vec3::Y);
        // Fade out below the horizon rather than snapping to black.
        light.illuminance = 11_000.0 * direction.y.max(0.0).powf(0.4);
    }
}

pub(crate) fn position_celestial_bodies(
    environments: Query<&DayEnvironment>,
    mut bodies: Query<(&CelestialBody, &mut Transform, &mut Visibility)>,
) {
    let Ok(environment) = environments.single() else { return };
    let angle = (environment.hour / 24.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
    for (body, mut transform, mut visibility) in &mut bodies {
        let phase = if body.moon { angle + std::f32::consts::PI } else { angle };
        let direction = Vec3::new(phase.cos(), phase.sin(), 0.25).normalize();
        transform.translation = direction * 400.0;
        *visibility = if direction.y > -0.05 { Visibility::Inherited } else { Visibility::Hidden };
    }
}

// ---------------------------------------------------------------------------
// Scenes outside the map (the Trick Guide demo set, trick_guide_scene.rs)
// ---------------------------------------------------------------------------

impl SceneCommands {
    /// Apply the queued spawns now. Every entity gets `MapEntity`; the caller re-tags what it spawned.
    pub(crate) fn apply_now(&mut self, world: &mut World) {
        std::mem::take(&mut self.queue).apply(world);
    }
}

impl<A: Asset> StagedAssets<A> {
    pub(crate) fn staged(world: &World) -> Self {
        Self::new(world)
    }
    pub(crate) fn publish_now(&mut self, world: &mut World) {
        self.publish(world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slice_takes_assets_in_order_up_to_the_budget_and_at_least_one() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let mut staged = StagedAssets::<Image>::new(&world);
        let sized = |bytes: usize| { let mut image = Image::default(); image.data = Some(vec![0; bytes]); image };
        let handles: Vec<_> = [40, 30, 100, 10].into_iter().map(|b| staged.add(sized(b))).collect();
        let mut assets = world.resource_mut::<Assets<Image>>();
        let inserted = |a: &Assets<Image>| handles.iter().map(|h| a.contains(h)).collect::<Vec<_>>();
        assert_eq!(staged.publish_some(&mut assets, 60, image_bytes), 70, "the asset that crosses the budget still goes in");
        assert_eq!(inserted(&assets), [true, true, false, false]);
        assert_eq!(staged.publish_some(&mut assets, 50, image_bytes), 100, "one asset over the budget goes in alone");
        assert_eq!(staged.publish_some(&mut assets, 50, image_bytes), 10);
        assert_eq!(inserted(&assets), [true; 4]);
        assert!(staged.pending.is_empty() && staged.owned.len() == 4, "inserted assets are owned (released with the map)");
    }
}
