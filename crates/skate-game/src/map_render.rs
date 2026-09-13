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
    fn new(world: &World) -> Self {
        Self {
            provider: world.resource::<Assets<A>>().get_handle_provider(),
            pending: Vec::new(),
            owned: Vec::new(),
        }
    }

    fn publish(&mut self, world: &mut World) {
        let mut assets = world.resource_mut::<Assets<A>>();
        for (handle, asset) in self.pending.drain(..) {
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
}

/// Assets owned by the live scene, released on the next map change.
#[derive(Resource, Default)]
pub(crate) struct MapAssets {
    groups: Vec<Box<dyn OwnedAssets>>,
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
        }
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
        });
        world.insert_resource(crate::map_transition::MapRenderBudget {
            draws: self.stats.draws,
            triangles: self.stats.triangles,
            leaves: self.stats.leaves,
            slabs: self.stats.slabs,
        });
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
