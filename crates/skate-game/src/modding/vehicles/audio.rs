//! Original synthesized exhaust sound, shared by all vehicle mods that opt in.
use bevy::{audio::{AddAudioSource, Volume}, prelude::*};
use super::engine_sound::Engine;

#[derive(Component)]
struct EngineVoice;
#[derive(Resource, Default)]
struct State { source: Option<Handle<Engine>>, pitch: f32, volume: f32 }

pub(super) fn install(app: &mut App) {
    app.add_audio_source::<Engine>().init_resource::<State>()
        .add_systems(Update, update.after(super::present));
}

fn update(
    mut commands: Commands, vehicles: Res<super::Vehicles>,
    time: Res<Time<Real>>, menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>, mut state: ResMut<State>,
    mut sources: ResMut<Assets<Engine>>, mut voices: Query<(Entity, &mut AudioSink), With<EngineVoice>>,
    pending: Query<Entity, With<EngineVoice>>,
) {
    let car = vehicles.driver.as_ref().and_then(|d| vehicles.owned.get(&(d.owner.clone(), d.key.clone())))
        .and_then(|i| vehicles.simulation.vehicles.get(&i.id))
        .filter(|c| c.definition.engine_audio.enabled);
    let active = crate::graphics_menu::gameplay_active(menu) && !replay.active;
    let (pitch, volume) = car.map_or((0.7, 0.), |car| {
        let a = &car.definition.engine_audio;
        let speed = (car.controller.current_vehicle_speed.abs() / car.definition.max_speed).clamp(0., 1.);
        let throttle = car.controls.throttle.abs();
        let revs = (speed * 0.65 + throttle * 0.35).clamp(0., 1.);
        (a.idle_pitch + (a.max_pitch - a.idle_pitch) * revs,
            if active { a.volume * (0.22 + 0.65 * throttle + 0.13 * speed) } else { 0. })
    });
    let dt = time.delta_secs().min(0.1);
    state.pitch += (pitch - state.pitch) * (1. - (-7. * dt).exp());
    state.volume += (volume - state.volume) * (1. - (-12. * dt).exp());
    if car.is_some() && pending.is_empty() {
        let source = state.source.get_or_insert_with(|| sources.add(Engine)).clone();
        commands.spawn((EngineVoice, AudioPlayer(source), PlaybackSettings::ONCE
            .with_volume(Volume::Linear(0.)).with_speed(pitch)));
        state.pitch = pitch;
    }
    for (entity, mut sink) in &mut voices {
        sink.set_speed(state.pitch.max(0.25));
        sink.set_volume(Volume::Linear(state.volume));
        if car.is_none() && state.volume < 0.001 { commands.entity(entity).despawn(); }
    }
}

