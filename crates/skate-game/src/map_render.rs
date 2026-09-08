//! CPU scene construction can run off-thread using the live asset allocators.
//! Only publishing assets/entities and retiring the previous scene touch the ECS.
use bevy::{asset::{AssetHandleProvider, AssetId}, ecs::world::CommandQueue, prelude::*};
use crate::retail_render::{RetailSkyMaterial, RetailWorldMaterial};

#[derive(Component)]
pub(crate) struct MapEntity;

pub(crate) trait AssetSink<A: Asset> {
    fn add(&mut self, asset: A) -> Handle<A>;
}
impl<A: Asset> AssetSink<A> for Assets<A> {
    fn add(&mut self, asset: A) -> Handle<A> { Assets::add(self, asset) }
}

pub(crate) struct StagedAssets<A: Asset> {
    provider: AssetHandleProvider,
    values: Vec<(Handle<A>, A)>,
}
impl<A: Asset> StagedAssets<A> {
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (AssetId<A>, &mut A)> {
        self.values.iter_mut().map(|(h, a)| (h.id(), a))
    }
    fn new(world: &World) -> Self {
        Self { provider: world.resource::<Assets<A>>().get_handle_provider(), values: Vec::new() }
    }
    fn publish(self, world: &mut World) -> Vec<AssetId<A>> {
        let mut assets = world.resource_mut::<Assets<A>>();
        self.values.into_iter().map(|(handle, asset)| {
            let id = handle.id();
            // Reserved from this Assets' allocator; the staged strong handle is
            // alive throughout preparation, so its generation cannot be reused.
            assets.insert(id, asset).expect("reserved map asset generation");
            id
        }).collect()
    }
}
impl<A: Asset> AssetSink<A> for StagedAssets<A> {
    fn add(&mut self, asset: A) -> Handle<A> {
        let handle = self.provider.reserve_handle().typed::<A>();
        self.values.push((handle.clone(), asset));
        handle
    }
}

/// Deferred spawns allocate their ECS identities only when the scene commits.
#[derive(Default)]
pub(crate) struct SceneCommands(CommandQueue);
pub(crate) struct SceneEntity<'a> {
    commands: &'a mut CommandQueue,
    id: std::sync::Arc<std::sync::OnceLock<Entity>>,
}
impl SceneCommands {
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> SceneEntity<'_> {
        let id = std::sync::Arc::new(std::sync::OnceLock::new());
        let spawned = id.clone();
        self.0.push(move |world: &mut World| {
            spawned.set(world.spawn((MapEntity, bundle)).id()).unwrap();
        });
        SceneEntity { commands: &mut self.0, id }
    }
    pub fn insert_resource<R: Resource>(&mut self, resource: R) {
        self.0.push(move |world: &mut World| { world.insert_resource(resource); });
    }
}
impl SceneEntity<'_> {
    pub fn insert<B: Bundle>(&mut self, bundle: B) {
        let id = self.id.clone();
        self.commands.push(move |world: &mut World| {
            world.entity_mut(*id.get().expect("scene spawn precedes insert")).insert(bundle);
        });
    }
}

pub(crate) struct PreparedScene {
    commands: SceneCommands,
    meshes: StagedAssets<Mesh>,
    materials: StagedAssets<StandardMaterial>,
    retail: StagedAssets<RetailWorldMaterial>,
    sky: StagedAssets<RetailSkyMaterial>,
    images: StagedAssets<Image>,
}
impl PreparedScene {
    pub fn new(world: &World) -> Self {
        Self { commands: Default::default(), meshes: StagedAssets::new(world),
            materials: StagedAssets::new(world), retail: StagedAssets::new(world),
            sky: StagedAssets::new(world), images: StagedAssets::new(world) }
    }
    pub fn prepare(&mut self, map: Option<&skate_data::skate_map::SkateMap>, root: &std::path::Path) {
        // Every scene starts with the same environment defaults. Map-specific
        // resources below overwrite these, including after a native scene.
        self.commands.insert_resource(ClearColor(Color::srgb(0.065, 0.08, 0.10)));
        self.commands.insert_resource(GlobalAmbientLight {
            color: Color::WHITE, brightness: 350., ..default()
        });
        if let Some(map) = map {
            crate::skate_world::spawn(map, &mut self.commands, &mut self.meshes,
                &mut self.materials, &mut self.retail, &mut self.images, &crate::retail_render::MaterialTuning::load(root));
            crate::retail_render::spawn_backdrop(&map.name, root, &mut self.commands, &mut self.meshes, &mut self.materials, &mut self.retail, &mut self.images);
            crate::retail_render::spawn_sky(&map.name, root, &mut self.commands,
                &mut self.meshes, &mut self.images, &mut self.sky, &mut self.retail);
        } else {
            crate::world::spawn_test_world(&mut self.commands, &mut self.meshes, &mut self.materials);
        }
    }
    pub fn publish(mut self, world: &mut World) {
        let owned = MapAssets {
            meshes: self.meshes.publish(world), materials: self.materials.publish(world),
            retail: self.retail.publish(world), sky: self.sky.publish(world),
            images: self.images.publish(world),
        };
        self.commands.0.apply(world);
        world.insert_resource(owned);
    }
}

#[derive(Resource, Default)]
pub(crate) struct MapAssets {
    meshes: Vec<AssetId<Mesh>>,
    materials: Vec<AssetId<StandardMaterial>>,
    retail: Vec<AssetId<RetailWorldMaterial>>,
    sky: Vec<AssetId<RetailSkyMaterial>>,
    images: Vec<AssetId<Image>>,
}
impl MapAssets {
    pub fn retire(world: &mut World) {
        let entities: Vec<_> = world.query_filtered::<Entity, With<MapEntity>>().iter(world).collect();
        for entity in entities { world.despawn(entity); }
        if let Some(owned) = world.remove_resource::<Self>() {
            fn remove<A: Asset>(world: &mut World, ids: Vec<AssetId<A>>) {
                let mut assets = world.resource_mut::<Assets<A>>();
                for id in ids { assets.remove(id); }
            }
            remove(world, owned.meshes);
            remove(world, owned.materials);
            remove(world, owned.retail);
            remove(world, owned.sky);
            remove(world, owned.images);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<RetailWorldMaterial>>();
        world.init_resource::<Assets<RetailSkyMaterial>>();
        world.init_resource::<Assets<Image>>();
        world
    }

    #[test]
    fn repeated_scene_retirement_releases_owned_assets_and_preserves_character() {
        let mut world = world();
        let character_mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let character = world.spawn((crate::world::PlayerRoot, Mesh3d(character_mesh.clone()))).id();
        let map = skate_data::skate_map::SkateMap::parse(include_bytes!("../../../maps/format-demo.skate")).unwrap();
        for iteration in 0..6 {
            let mut scene = PreparedScene::new(&world);
            scene.prepare((iteration % 2 == 0).then_some(&map), std::path::Path::new("unused"));
            // Preparation has not published any live entities or assets.
            assert_eq!(world.resource::<Assets<Mesh>>().len(), 1);
            scene.publish(&mut world);
            assert!(world.query_filtered::<Entity, With<MapEntity>>().iter(&world).count() > 0);
            let old: Vec<_> = world.query_filtered::<Entity, With<MapEntity>>().iter(&world).collect();
            MapAssets::retire(&mut world);
            assert!(old.into_iter().all(|id| world.get_entity(id).is_err()));
            assert!(world.get_entity(character).is_ok());
            assert_eq!(world.resource::<Assets<Mesh>>().len(), 1);
            assert!(world.resource::<Assets<Mesh>>().contains(&character_mesh));
            assert_eq!(world.resource::<Assets<StandardMaterial>>().len(), 0);
            assert_eq!(world.resource::<Assets<RetailWorldMaterial>>().len(), 0);
            assert_eq!(world.resource::<Assets<RetailSkyMaterial>>().len(), 0);
            assert_eq!(world.resource::<Assets<Image>>().len(), 0);
        }
    }

    #[test]
    fn abandoned_preparation_does_not_replace_live_scene() {
        let mut world = world();
        let mut active = PreparedScene::new(&world);
        active.prepare(None, std::path::Path::new("unused"));
        active.publish(&mut world);
        let entities: Vec<_> = world.query_filtered::<Entity, With<MapEntity>>().iter(&world).collect();
        let meshes = world.resource::<Assets<Mesh>>().len();
        let mut abandoned = PreparedScene::new(&world);
        abandoned.prepare(None, std::path::Path::new("unused"));
        drop(abandoned);
        assert_eq!(world.resource::<Assets<Mesh>>().len(), meshes);
        assert!(entities.into_iter().all(|id| world.get_entity(id).is_ok()));
    }
}
