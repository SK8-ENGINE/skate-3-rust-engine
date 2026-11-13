//! Per-actor component tracks (world audio state) recorded and written back in playback (b375, b378 A1..A3).
//! Details: docs/hails-additions/37-instant-replay.md#replay-component-tracks
use super::{
    Replay,
    world::{ReplayActor, ReplayGhost},
};
use bevy::prelude::*;
use std::collections::{HashMap, VecDeque};

/// Tracks are trimmed to the window this often (frames).
const TRIM_EVERY: u32 = 60;

#[derive(Resource)]
struct ComponentTracks<T> {
    tracks: HashMap<u64, VecDeque<(f64, T)>>,
    /// Live values of the entities playback wrote, put back on exit.
    saved: Option<HashMap<Entity, T>>,
    last: Option<f64>,
    since_trim: u32,
}

impl<T> Default for ComponentTracks<T> {
    fn default() -> Self {
        Self { tracks: HashMap::new(), saved: None, last: None, since_trim: 0 }
    }
}

impl<T: Clone> ComponentTracks<T> {
    /// The value holding at `time` (None before the actor's first one).
    fn at(&self, id: u64, time: f64) -> Option<&T> {
        let track = self.tracks.get(&id)?;
        let i = track.partition_point(|(t, _)| *t <= time).checked_sub(1)?;
        Some(&track[i].1)
    }
}

/// Record `T` with the actors and play it back.
pub(super) fn track<T: Component<Mutability = bevy::ecs::component::Mutable> + Clone + PartialEq>(app: &mut App) {
    app.init_resource::<ComponentTracks<T>>()
        .add_systems(PostUpdate, record::<T>.after(super::world::record))
        .add_systems(PreUpdate, play::<T>.after(super::controls));
}

fn record<T: Component + Clone + PartialEq>(
    replay: Res<Replay>,
    mut store: ResMut<ComponentTracks<T>>,
    actors: Query<(&ReplayActor, &T), Without<ReplayGhost>>,
) {
    let Some(now) = replay.world_time.filter(|_| !replay.active) else { return };
    if store.last == Some(now) {
        return;
    }
    if store.last.is_some_and(|last| now < last) {
        store.tracks.clear();
    }
    store.last = Some(now);
    for (actor, value) in &actors {
        let track = store.tracks.entry(actor.id).or_default();
        if track.back().is_none_or(|(_, v)| v != value) {
            track.push_back((now, value.clone()));
        }
    }
    store.since_trim += 1;
    if store.since_trim >= TRIM_EVERY {
        store.since_trim = 0;
        let start = replay.world.bounds().map_or(now, |(start, _)| start);
        store.tracks.retain(|id, track| {
            while track.len() >= 2 && track[1].0 <= start {
                track.pop_front();
            }
            replay.world.contains(*id)
        });
    }
}

fn play<T: Component<Mutability = bevy::ecs::component::Mutable> + Clone + PartialEq>(
    replay: Res<Replay>,
    mut store: ResMut<ComponentTracks<T>>,
    mut actors: Query<(Entity, &ReplayActor, &mut T)>,
) {
    if !replay.active {
        for (entity, value) in store.saved.take().into_iter().flatten() {
            if let Ok((_, _, mut current)) = actors.get_mut(entity) {
                *current = value;
            }
        }
        return;
    }
    let store = &mut *store;
    let saved = store.saved.get_or_insert_with(HashMap::new);
    for (entity, actor, mut current) in &mut actors {
        saved.entry(entity).or_insert_with(|| current.clone());
        let tracks = &store.tracks;
        let value = tracks.get(&actor.id).and_then(|track| {
            let i = track.partition_point(|(t, _)| *t <= replay.cursor).checked_sub(1)?;
            Some(&track[i].1)
        });
        if let Some(value) = value.filter(|v| *v != &*current) {
            *current = value.clone();
        }
    }
}

/// [`track`], and a ghost (an actor that left inside the window) carries `T` while playback shows it, so its sound
/// plays again (retail rebuilds every recorded actor from the frame, live or gone, b670); never in live play.
pub(super) fn track_on_ghosts<T: Component<Mutability = bevy::ecs::component::Mutable> + Clone + PartialEq>(app: &mut App) {
    track::<T>(app);
    app.add_systems(PreUpdate, ghost_values::<T>.after(play::<T>));
}

fn ghost_values<T: Component<Mutability = bevy::ecs::component::Mutable> + Clone + PartialEq>(
    replay: Res<Replay>,
    store: Res<ComponentTracks<T>>,
    ghosts: Query<(Entity, &ReplayActor, Has<T>), With<ReplayGhost>>,
    mut commands: Commands,
) {
    for (entity, actor, has) in &ghosts {
        let shown = replay.active && replay.world.shown(actor.id, replay.cursor);
        match (shown.then(|| store.at(actor.id, replay.cursor)).flatten(), has) {
            (Some(value), false) => {
                commands.entity(entity).insert(value.clone());
            }
            (None, true) => {
                commands.entity(entity).remove::<T>();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
pub(super) fn value_at<T: Component + Clone + Send + Sync + 'static>(world: &World, id: u64, time: f64) -> Option<T> {
    world.resource::<ComponentTracks<T>>().at(id, time).cloned()
}
