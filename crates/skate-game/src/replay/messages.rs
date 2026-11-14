//! One-shot messages recorded and re-sent when playback crosses them (b375, b378 A4); speech not recorded.
//! Details: docs/hails-additions/37-instant-replay.md#replay-one-shot-messages
use super::Replay;
use bevy::{ecs::message::Message, prelude::*};
use std::collections::VecDeque;

/// At most this many recorded messages of one kind are kept (a busy 90 s window holds far fewer).
const MAX_MESSAGES: usize = 4096;

#[derive(Resource)]
struct MessageTrack<M> {
    messages: VecDeque<(f64, M)>,
    /// The cursor the playback last wrote up to.
    played: Option<f64>,
}

impl<M> Default for MessageTrack<M> {
    fn default() -> Self {
        Self { messages: VecDeque::new(), played: None }
    }
}

pub(super) fn track<M: Message + Clone>(app: &mut App) {
    app.init_resource::<MessageTrack<M>>()
        .add_systems(PostUpdate, record::<M>)
        .add_systems(PreUpdate, play::<M>.after(super::controls));
}

/// Live messages go in at the newest frame's time; during playback the reader is drained (those are the replayed
/// ones).
fn record<M: Message + Clone>(replay: Res<Replay>, mut track: ResMut<MessageTrack<M>>, mut reader: MessageReader<M>) {
    if replay.active {
        reader.clear();
        return;
    }
    let Some(now) = replay.frames.back().map(|f| f.time) else {
        reader.clear();
        return;
    };
    if track.messages.back().is_some_and(|(t, _)| *t > now) {
        track.messages.clear();
    }
    track.messages.extend(reader.read().map(|m| (now, m.clone())));
    let start = replay.bounds().0;
    while track.messages.front().is_some_and(|(t, _)| *t < start) || track.messages.len() > MAX_MESSAGES {
        track.messages.pop_front();
    }
}

/// Write the messages whose time the cursor crossed going forward since the last frame.
fn play<M: Message + Clone>(replay: Res<Replay>, mut track: ResMut<MessageTrack<M>>, mut writer: MessageWriter<M>) {
    if !replay.active {
        track.played = None;
        return;
    }
    let cursor = replay.cursor;
    let Some(previous) = track.played.replace(cursor) else { return };
    if cursor <= previous {
        return;
    }
    let from = track.messages.partition_point(|(t, _)| *t <= previous);
    let to = track.messages.partition_point(|(t, _)| *t <= cursor);
    writer.write_batch(track.messages.range(from..to).map(|(_, m)| m.clone()));
}
