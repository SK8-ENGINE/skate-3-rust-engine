use super::*;

fn pose(x: f32) -> Snapshot {
    Snapshot {
        root: Transform::from_xyz(x, 0., 0.),
        bones: vec![Mat4::from_translation(Vec3::X * x)],
        camera: Transform::from_xyz(x, 2., 3.),
        fov: 1.0,
    }
}

#[test]
fn back_is_refused_while_the_fe_fade_shows() {
    assert!(back_opens(true, None));
    assert!(back_opens(true, Some(0.0)));
    assert!(!back_opens(true, Some(0.04)), "fading");
    assert!(!back_opens(true, Some(1.0)), "black");
    assert!(!back_opens(false, Some(0.0)), "not enough recorded");
}

#[test]
fn rolling_window_is_the_retail_ninety_seconds_with_variable_cadence() {
    let mut replay = Replay::default();
    for i in 0..4000 {
        replay.record(
            pose(i as f32),
            if i % 2 == 0 { 1. / 60. } else { 1. / 30. },
            false,
        );
    }
    let (start, end) = replay.bounds();
    // 5400 frames at retail's 60 per second (b345).
    assert!((end - start - 90.).abs() < 1e-6);
    assert!(replay.frames.len() <= 3602);
    replay.enter();
    replay.seek(-100.);
    assert_eq!(replay.cursor, start);
    replay.seek(1000.);
    assert_eq!(replay.cursor, end);
}

#[test]
fn scrubbing_interpolates_shared_pose_and_camera_without_recording() {
    let mut replay = Replay::default();
    replay.enter();
    assert!(!replay.active);
    replay.record(pose(0.), 0.1, false);
    replay.record(pose(1.), 0.1, false);
    replay.enter();
    replay.record(pose(99.), 0.1, false);
    assert_eq!(replay.frames.len(), 2);
    replay.seek(0.05);
    let (a, b, alpha) = replay.sample().unwrap();
    assert_eq!(alpha, 0.5);
    assert_eq!(blend(a.root, b.root, alpha).translation.x, 0.5);
    assert_eq!(blend(a.camera, b.camera, alpha).translation.x, 0.5);
    replay.toggle_camera();
    let free = replay.free_camera.unwrap();
    replay.seek(0.1);
    assert_eq!(replay.free_camera.unwrap(), free);
    replay.toggle_camera();
    assert!(replay.free_camera.is_none());
    replay.exit();
    assert!(replay.sample().is_none());
    replay.record(pose(2.), 0.1, false);
    assert_eq!(replay.frames.back().unwrap().pose.root.translation.x, 2.);
}

#[test]
fn frame_steps_and_cuts_do_not_blend_through_teleports() {
    let mut replay = Replay::default();
    replay.record(pose(0.), 0.1, false);
    replay.record(pose(100.), 0.1, true);
    replay.record(pose(101.), 0.1, false);
    replay.enter();
    replay.seek(0.05);
    let (a, b, _) = replay.sample().unwrap();
    assert_eq!(a.root, b.root);
    assert_eq!(a.root.translation.x, 0.);
    replay.step(1);
    assert_eq!(replay.sample().unwrap().0.root.translation.x, 100.);
    replay.step(-1);
    assert_eq!(replay.cursor, 0.);
}

#[test]
fn replay_gates_all_fixed_gameplay_sets() {
    #[derive(Resource, Default)]
    struct Counts([usize; 3]);
    let mut app = App::new();
    app.init_resource::<Replay>()
        .init_resource::<Counts>()
        .configure_sets(
            FixedUpdate,
            (
                SimulationSet::Input,
                SimulationSet::Controls,
                SimulationSet::Physics,
            )
                .run_if(live),
        )
        .add_systems(
            FixedUpdate,
            (
                (|mut c: ResMut<Counts>| c.0[0] += 1).in_set(SimulationSet::Input),
                (|mut c: ResMut<Counts>| c.0[1] += 1).in_set(SimulationSet::Controls),
                (|mut c: ResMut<Counts>| c.0[2] += 1).in_set(SimulationSet::Physics),
            ),
        );
    app.world_mut().run_schedule(FixedUpdate);
    app.world_mut().resource_mut::<Replay>().active = true;
    for _ in 0..120 {
        app.world_mut().run_schedule(FixedUpdate);
    }
    assert_eq!(app.world().resource::<Counts>().0, [1; 3]);
    app.world_mut().resource_mut::<Replay>().exit();
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(app.world().resource::<Counts>().0, [2; 3]);
}

#[test]
fn controller_replay_scrub_camera_and_exit_do_not_leak_gameplay_input() {
    use crate::input::ControllerInput;
    use skate_core::input::xbox::XboxState;
    let mut app = App::new();
    app.init_resource::<Replay>()
        .init_resource::<ControllerInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
        .init_resource::<Time<Real>>()
        .init_resource::<crate::popup_host::PopupMods>()
        .add_message::<crate::ui_audio::FrontendSound>()
        .add_systems(Update, controls);
    // 6.7 s: over retail's 5 s minimum.
    for i in 0..400 {
        app.world_mut()
            .resource_mut::<Replay>()
            .record(pose(i as f32), 1. / 60., false);
    }
    let tick = |app: &mut App, buttons, triggers, left| {
        app.world_mut()
            .resource_mut::<ControllerInput>()
            .sample_raw_for_test(XboxState {
                buttons,
                triggers,
                left,
                right: [0; 2],
            });
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs_f64(1. / 60.));
        app.world_mut().run_schedule(Update);
    };
    tick(&mut app, SELECT, [0; 2], [0; 2]);
    assert!(app.world().resource::<Replay>().active);
    tick(&mut app, SELECT, [0; 2], [0; 2]);
    assert!(
        app.world().resource::<Replay>().active,
        "held View must not toggle repeatedly"
    );
    let end = app.world().resource::<Replay>().cursor;
    // The scrub moves on whole console frames (1/30 s = two of these ticks) and eases in (ScrubSpec).
    tick(&mut app, 0, [255, 0], [0; 2]);
    tick(&mut app, 0, [255, 0], [0; 2]);
    let rewound = app.world().resource::<Replay>().cursor;
    assert!(rewound < end);
    // Forward: the rate eases from the rewind through zero, then moves past where the rewind stopped.
    for _ in 0..16 {
        tick(&mut app, 0, [0, 128], [0; 2]);
    }
    assert!(app.world().resource::<Replay>().cursor > rewound);
    // Retail: D-pad Left jumps to the start; RB plays and stops; a stick resumes a stopped replay.
    // Let the scrub ease out (a moving scrub stops playback, as retail's rate does).
    for _ in 0..40 {
        tick(&mut app, 0, [0; 2], [0; 2]);
    }
    tick(&mut app, DPAD_LEFT, [0; 2], [0; 2]);
    let start = app.world().resource::<Replay>().bounds().0;
    assert_eq!(app.world().resource::<Replay>().cursor, start);
    tick(&mut app, RB, [0; 2], [0; 2]);
    assert!(app.world().resource::<Replay>().playing);
    tick(&mut app, 0, [0; 2], [0; 2]);
    tick(&mut app, RB, [0; 2], [0; 2]);
    assert!(!app.world().resource::<Replay>().playing);
    tick(&mut app, 0, [0; 2], [20000, 0]);
    assert!(app.world().resource::<Replay>().playing, "a stick resumes playback");
    tick(&mut app, DPAD_RIGHT, [0; 2], [0; 2]);
    let replay = app.world().resource::<Replay>();
    assert!(!replay.playing && replay.cursor == replay.bounds().1);
    // Without the retail menu stage, B (Finished) leaves when it is released.
    tick(&mut app, B, [255, 0], [32767, 0]);
    assert!(app.world().resource::<Replay>().active, "not while B is held");
    tick(&mut app, 0, [255, 0], [32767, 0]);
    assert!(!app.world().resource::<Replay>().active);
    assert_eq!(
        app.world().resource::<ControllerInput>().mapped_actions,
        [[0.; 18]; 4]
    );
    tick(&mut app, 0, [255, 0], [32767, 0]);
    assert!(app.world().resource::<Replay>().release_controls);
    assert_eq!(
        app.world().resource::<ControllerInput>().mapped_actions,
        [[0.; 18]; 4]
    );
    tick(&mut app, 0, [0; 2], [0; 2]);
    assert!(!app.world().resource::<Replay>().release_controls);
    tick(&mut app, 0x1000, [0; 2], [0; 2]);
    assert_ne!(
        app.world().resource::<ControllerInput>().mapped_actions,
        [[0.; 18]; 4]
    );
}

#[test]
fn world_actors_play_back_from_the_recording_and_return_to_live_on_exit() {
    use super::world::ReplayActor;
    let mut app = App::new();
    app.init_resource::<Replay>();
    super::world::plugin(&mut app);
    let joint = app.world_mut().spawn(Transform::from_xyz(0., 1., 0.)).id();
    let car = app.world_mut().spawn((ReplayActor { id: 9 }, super::world::ReplayJoints(vec![joint]),
        Transform::from_xyz(0., 0., 0.), Visibility::Inherited)).id();
    let step = |app: &mut App, x: f32| {
        app.world_mut().resource_mut::<Replay>().record(pose(x), 1. / 60., false);
        app.update();
    };
    step(&mut app, 0.);
    app.world_mut().entity_mut(car).insert(Transform::from_xyz(2., 0., 0.));
    app.world_mut().entity_mut(joint).insert(Transform::from_xyz(0., 1., 0.).with_rotation(Quat::from_rotation_x(1.0)));
    step(&mut app, 1.);
    // An actor that appeared after the first frame is not drawn before it.
    let late = app.world_mut().spawn((ReplayActor { id: 10 }, Transform::from_xyz(5., 0., 0.), Visibility::Inherited)).id();
    step(&mut app, 2.);
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(0.5 / 60.);
    }
    app.update();
    let world = app.world();
    assert!((world.get::<Transform>(car).unwrap().translation.x - 1.).abs() < 1e-4);
    let angle = world.get::<Transform>(joint).unwrap().rotation.angle_between(Quat::from_rotation_x(0.5));
    assert!(angle < 2e-3, "{angle}");
    assert_eq!(world.get::<Visibility>(late), Some(&Visibility::Hidden));
    assert_eq!(world.get::<Visibility>(car), Some(&Visibility::Inherited));
    app.world_mut().resource_mut::<Replay>().exit();
    app.update();
    let world = app.world();
    assert_eq!(world.get::<Transform>(car).unwrap().translation.x, 2.);
    assert_eq!(world.get::<Visibility>(late), Some(&Visibility::Inherited));
}

#[test]
fn mod_markers_and_limits_are_owned_and_go_with_their_mod() {
    let mut replay = Replay::default();
    assert!(replay.add_marker("a", "early".into()).is_err(), "nothing recorded yet");
    replay.record(pose(0.), 1. / 60., false);
    replay.add_marker("a", "kickflip".into()).unwrap();
    replay.add_marker("b", "bail".into()).unwrap();
    for _ in 2..MAX_MARKERS {
        replay.add_marker("b", "x".into()).unwrap();
    }
    assert!(replay.add_marker("a", "one too many".into()).is_err());
    replay.set_limits("a", Some(1800), None, None);
    assert_eq!(replay.world.limits.max_frames, 1800);
    assert!((replay.window() - 30.).abs() < 1e-9);
    // Another mod's reset does not drop a's limits; a's disable does.
    replay.set_limits("b", None, None, None);
    assert_eq!(replay.world.limits.max_frames, 1800);
    replay.clear_owner("a");
    assert_eq!(replay.world.limits.max_frames, 5400);
    // A mod's share limit (b470 item 5) reaches the editor's limits; its disable restores retail's 60 s.
    replay.set_limits("a", None, None, Some(120));
    assert_eq!(replay.edit_limits.share_max_seconds, 120);
    replay.clear_owner("a");
    assert_eq!(replay.edit_limits.share_max_seconds, 60);
    let state = replay.state();
    assert_eq!(state["markers"].as_array().unwrap().len(), MAX_MARKERS - 1);
    assert_eq!(state["max_frames"], 5400);
    replay.clear_mods();
    assert_eq!(replay.state()["markers"].as_array().unwrap().len(), 0);
    // Markers leave with the window.
    replay.add_marker("a", "old".into()).unwrap();
    for _ in 0..(91 * 60) {
        replay.record(pose(1.), 1. / 60., false);
    }
    assert_eq!(replay.state()["markers"].as_array().unwrap().len(), 0);
}

#[test]
fn a_despawned_actor_plays_back_as_a_ghost_until_it_leaves_the_window() {
    use super::world::{ReplayActor, ReplayGhost, ReplayJoints};
    use bevy::ecs::system::RunSystemOnce;
    let mut app = App::new();
    app.init_resource::<Replay>();
    app.world_mut().resource_mut::<Replay>().world.limits.max_frames = 60;
    super::world::plugin(&mut app);
    let ped = app.world_mut().spawn((Transform::from_xyz(3., 0., 0.), Visibility::Inherited)).id();
    let scene = app.world_mut().spawn((ChildOf(ped), Transform::default())).id();
    let joint = app.world_mut().spawn((ChildOf(scene), Transform::from_xyz(0., 1., 0.))).id();
    app.world_mut().entity_mut(ped).insert((ReplayActor { id: 1 << 56 | 5 }, ReplayJoints(vec![joint])));
    let step = |app: &mut App| {
        app.world_mut().resource_mut::<Replay>().record(pose(0.), 1. / 60., false);
        app.update();
    };
    step(&mut app);
    step(&mut app);
    let fade = crate::living_world::npc_skaters::NpcFade { alpha: 0.25, ..Default::default() };
    app.world_mut().entity_mut(ped).insert(fade);
    app.world_mut().run_system_once(move |mut commands: Commands| super::world::retire(&mut commands, ped)).unwrap();
    assert!(app.world().get_entity(ped).is_err());
    let ghost = app.world_mut().query_filtered::<Entity, With<ReplayGhost>>().single(app.world()).unwrap();
    assert_eq!(app.world().get::<ChildOf>(scene).unwrap().parent(), ghost);
    assert_eq!(app.world().get::<crate::living_world::npc_skaters::NpcFade>(ghost), Some(&fade), "the fade goes with the scene");
    step(&mut app);
    step(&mut app);
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(0.);
    }
    app.update();
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Inherited));
    assert_eq!(app.world().get::<Transform>(ghost).unwrap().translation.x, 3.);
    app.world_mut().resource_mut::<Replay>().seek(3. / 60.);
    app.update();
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden));
    app.world_mut().resource_mut::<Replay>().exit();
    app.update();
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden), "hidden while live");
    for _ in 0..200 {
        step(&mut app);
    }
    assert!(app.world().get_entity(ghost).is_err() && app.world().get_entity(joint).is_err());
}

/// A ped's held prop (its own actor under the ped's scene) stays in the ghost's hand after the ped despawns.
#[test]
fn a_ghost_ped_keeps_its_held_prop() {
    use super::world::{HAND_PROP_TAG, ReplayActor, ReplayGhost, ReplayJoints};
    use bevy::ecs::system::RunSystemOnce;
    let mut app = App::new();
    app.init_resource::<Replay>();
    super::world::plugin(&mut app);
    let ped = app.world_mut().spawn((Transform::from_xyz(3., 0., 0.), Visibility::Inherited)).id();
    let scene = app.world_mut().spawn((ChildOf(ped), Transform::default(), Visibility::Inherited)).id();
    let joint = app.world_mut().spawn((ChildOf(scene), Transform::from_xyz(0., 1., 0.))).id();
    let prop = app.world_mut().spawn((ChildOf(scene), Transform::from_xyz(0.2, 1., 0.), Visibility::Inherited)).id();
    app.world_mut().entity_mut(ped).insert((ReplayActor { id: 1 << 56 | 6 }, ReplayJoints(vec![joint])));
    app.world_mut().entity_mut(prop).insert(ReplayActor { id: HAND_PROP_TAG | 1 });
    let step = |app: &mut App| {
        app.world_mut().resource_mut::<Replay>().record(pose(0.), 1. / 60., false);
        app.update();
    };
    step(&mut app);
    step(&mut app);
    app.world_mut().run_system_once(move |mut commands: Commands| super::world::retire(&mut commands, ped)).unwrap();
    let ghost = app.world_mut().query_filtered::<Entity, With<ReplayGhost>>().single(app.world()).unwrap();
    step(&mut app);
    step(&mut app);
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(0.);
    }
    app.update();
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Inherited));
    assert!(app.world().get_entity(prop).is_ok(), "the prop still exists");
    assert_eq!(app.world().get::<ChildOf>(prop).map(ChildOf::parent), Some(scene));
    assert_ne!(app.world().get::<Visibility>(prop), Some(&Visibility::Hidden), "drawn in the ghost's hand");
    assert_eq!(app.world().get::<Transform>(prop).unwrap().translation, Vec3::new(0.2, 1., 0.));
}

#[test]
fn playback_publishes_the_recorded_audio_steps_the_cursor_crosses() {
    use crate::game_audio::skate_events::Riding;
    let mut replay = Replay::default();
    for i in 0..30 {
        replay.record(pose(i as f32), 1. / 60., false);
        let mut riding = Riding { speed: i as f32, ..Riding::default() };
        riding.audio.push_trigger = i == 3;
        replay.attach_riding(riding);
        // A second attach in the same frame does not replace the frame's step.
        replay.attach_riding(Riding { speed: -1., ..Riding::default() });
    }
    replay.enter();
    replay.seek(0.);
    assert!(replay.audio_steps().is_empty(), "entering publishes nothing");
    assert!(replay.jumped() && !replay.audible());
    replay.seek(4. / 60.);
    let steps = replay.audio_steps();
    assert_eq!(steps.iter().map(|r| r.speed).collect::<Vec<_>>(), [1., 2., 3., 4.]);
    assert!(steps[2].audio.push_trigger && replay.audible() && !replay.jumped());
    assert!(replay.audio_steps().is_empty() && !replay.audible(), "paused is silent");
    // Rewind: the step at the cursor, without its push.
    replay.seek(3. / 60.);
    let back = replay.audio_steps();
    assert_eq!(back.len(), 1);
    assert!(back[0].speed == 3. && !back[0].audio.push_trigger && replay.jumped() && !replay.audible());
    // A fast scrub keeps the newest steps and the skipped push.
    replay.seek(2. / 60.);
    replay.audio_steps();
    replay.seek(20. / 60.);
    let fast = replay.audio_steps();
    assert_eq!(fast.len(), MAX_FED_STEPS);
    assert_eq!(fast.last().unwrap().speed, 20.);
    assert!(fast[0].audio.push_trigger && replay.jumped());
    replay.exit();
    assert!(replay.audio_steps().is_empty() && !replay.audible());
}

#[test]
fn world_actor_audio_state_follows_the_recording_and_returns_on_exit() {
    use super::world::ReplayActor;
    use crate::world_audio::AudioVelocity;
    let mut app = App::new();
    app.init_resource::<Replay>();
    super::world::plugin(&mut app);
    super::components::track::<AudioVelocity>(&mut app);
    let car = app.world_mut().spawn((ReplayActor { id: 2 << 56 | 7 }, Transform::default(), AudioVelocity(Vec3::X))).id();
    let step = |app: &mut App| {
        app.world_mut().resource_mut::<Replay>().record(pose(0.), 1. / 60., false);
        app.update();
    };
    step(&mut app);
    step(&mut app);
    app.world_mut().entity_mut(car).insert(AudioVelocity(Vec3::Y * 5.));
    step(&mut app);
    // Unchanged values are not stored again.
    assert_eq!(super::components::value_at::<AudioVelocity>(app.world(), 2 << 56 | 7, 1.5 / 60.), Some(AudioVelocity(Vec3::X)));
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(1. / 60.);
    }
    app.update();
    assert_eq!(app.world().get::<AudioVelocity>(car), Some(&AudioVelocity(Vec3::X)));
    app.world_mut().resource_mut::<Replay>().seek(2. / 60.);
    app.update();
    assert_eq!(app.world().get::<AudioVelocity>(car), Some(&AudioVelocity(Vec3::Y * 5.)));
    app.world_mut().resource_mut::<Replay>().seek(0.);
    app.update();
    app.world_mut().resource_mut::<Replay>().exit();
    app.update();
    assert_eq!(app.world().get::<AudioVelocity>(car), Some(&AudioVelocity(Vec3::Y * 5.)), "live value back");
}

/// b670: a ghost (the actor left inside the window) carries its recorded audio state while playback shows it, and
/// never in live play.
#[test]
fn a_ghost_sounds_in_playback_while_it_is_in_the_frame() {
    use super::world::{ReplayActor, ReplayGhost};
    use crate::world_audio::AudioVelocity;
    let mut app = App::new();
    app.init_resource::<Replay>();
    super::world::plugin(&mut app);
    super::components::track_on_ghosts::<AudioVelocity>(&mut app);
    let id = 2 << 56 | 9;
    let car = app.world_mut().spawn((ReplayActor { id }, Transform::default(), AudioVelocity(Vec3::X))).id();
    let step = |app: &mut App| {
        app.world_mut().resource_mut::<Replay>().record(pose(0.), 1. / 60., false);
        app.update();
    };
    step(&mut app);
    step(&mut app);
    app.world_mut().entity_mut(car).despawn();
    let ghost = app.world_mut().spawn((ReplayGhost, ReplayActor { id }, Transform::default(), Visibility::Hidden)).id();
    step(&mut app);
    step(&mut app);
    assert!(app.world().get::<AudioVelocity>(ghost).is_none(), "live play: the ghost stays silent");
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(1. / 60.);
    }
    app.update();
    app.update();
    assert_eq!(app.world().get::<AudioVelocity>(ghost), Some(&AudioVelocity(Vec3::X)), "in the frame: recorded value");
    app.world_mut().resource_mut::<Replay>().seek(3. / 60.);
    app.update();
    app.update();
    assert!(app.world().get::<AudioVelocity>(ghost).is_none(), "after its despawn: gone");
    app.world_mut().resource_mut::<Replay>().seek(1. / 60.);
    app.update();
    app.update();
    assert!(app.world().get::<AudioVelocity>(ghost).is_some());
    app.world_mut().resource_mut::<Replay>().exit();
    app.update();
    app.update();
    assert!(app.world().get::<AudioVelocity>(ghost).is_none(), "exit: removed");
}

#[test]
fn one_shot_sounds_fire_again_when_playback_crosses_them() {
    use crate::world_audio::VehicleHorn;
    #[derive(Resource, Default)]
    struct Heard(Vec<u8>);
    let mut app = App::new();
    app.init_resource::<Replay>().init_resource::<Heard>().add_message::<VehicleHorn>();
    super::messages::track::<VehicleHorn>(&mut app);
    app.add_systems(Update, |mut r: MessageReader<VehicleHorn>, mut heard: ResMut<Heard>| {
        heard.0.extend(r.read().map(|h| h.kind));
    });
    let step = |app: &mut App, horn: Option<u8>| {
        app.world_mut().resource_mut::<Replay>().record(pose(0.), 1. / 60., false);
        if let Some(kind) = horn {
            app.world_mut().write_message(VehicleHorn { vehicle: Entity::PLACEHOLDER, kind, seconds: 1. });
        }
        app.update();
    };
    step(&mut app, None);
    step(&mut app, Some(4));
    step(&mut app, None);
    app.world_mut().resource_mut::<Heard>().0.clear();
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(0.);
    }
    app.update();
    app.world_mut().resource_mut::<Replay>().seek(2. / 60.);
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Heard>().0, [4], "once, when the cursor passed it");
    app.world_mut().resource_mut::<Replay>().seek(0.);
    app.update();
    assert_eq!(app.world().resource::<Heard>().0, [4], "not on a rewind");
}

#[test]
fn a_mod_camera_override_goes_on_restore_and_disable() {
    let mut replay = Replay::default();
    assert_eq!(replay.follow_eye, camera_key::FollowEye::Camera, "retail: the gameplay camera (m346)");
    replay.set_camera("m", Some(false), None, Some(80.), Some(camera_key::FollowEye::Skater), Some(false), Some(true));
    assert!(!replay.camera_collision.enabled && replay.camera_collision.leash == 80.);
    assert!(replay.camera_collision.playback, "playback_collide");
    assert_eq!(replay.camera_collision.radius, 0.1, "unset fields keep retail's");
    assert_eq!((replay.follow_eye, replay.fill_section), (camera_key::FollowEye::Skater, Some(false)));
    replay.set_camera("other", None, None, None, None, None, None);
    assert!(!replay.camera_collision.enabled, "only the owner restores");
    replay.clear_owner("m");
    assert_eq!(replay.camera_collision, camera_collision::CameraCollision::default());
    assert_eq!((replay.follow_eye, replay.fill_section), (camera_key::FollowEye::Camera, None));
}

#[test]
fn free_camera_stays_within_retail_forty_metres_of_the_player() {
    let player = Vec3::new(10., 0., 0.);
    let rules = Replay::default().camera_collision;
    assert_eq!(rules.leash, 40.);
    let leashed = |to: Vec3| camera_collision::constrain(Some(to), to, player, true, &rules, |_, _, _| None).position;
    assert_eq!(leashed(Vec3::new(20., 0., 0.)), Vec3::new(20., 0., 0.));
    assert!((leashed(Vec3::new(110., 0., 0.)) - Vec3::new(50., 0., 0.)).length() < 1e-4);
}

#[test]
fn retail_quit_warning_and_minimum_length() {
    use crate::input::ControllerInput;
    use skate_core::input::xbox::XboxState;
    let mut app = App::new();
    app.init_resource::<Replay>()
        .init_resource::<ControllerInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
        .init_resource::<Time<Real>>()
        .init_resource::<crate::popup_host::PopupMods>()
        .add_message::<crate::ui_audio::FrontendSound>()
        .add_systems(Update, controls);
    let tick = |app: &mut App, buttons| {
        app.world_mut().resource_mut::<ControllerInput>().sample_raw_for_test(XboxState { buttons, triggers: [0; 2], left: [0; 2], right: [0; 2] });
        app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_secs_f64(1. / 60.));
        app.world_mut().run_schedule(Update);
    };
    // Under 5 s recorded: Back is refused silently.
    for i in 0..200 {
        app.world_mut().resource_mut::<Replay>().record(pose(i as f32), 1. / 60., false);
    }
    tick(&mut app, SELECT);
    assert!(!app.world().resource::<Replay>().active);
    tick(&mut app, 0);
    for i in 200..400 {
        app.world_mut().resource_mut::<Replay>().record(pose(i as f32), 1. / 60., false);
    }
    tick(&mut app, SELECT);
    assert!(app.world().resource::<Replay>().active);
    // Retail's open sound (`editor_select`, b389).
    let sounds = app.world().resource::<bevy::ecs::message::Messages<crate::ui_audio::FrontendSound>>();
    assert!(sounds.iter_current_update_messages().any(|m| *m == crate::ui_audio::FrontendSound::named(OPEN_SOUND)));
    tick(&mut app, 0);
    // With the retail menu stage up, B raises the quit warning; No stays, Yes leaves.
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().stage_ready = true;
    tick(&mut app, B);
    // Not while B is held: the press would reach the popup and cancel it (desktop session 2026-10-20).
    assert!(app.world().resource::<crate::popup_host::PopupMods>().engine_requests.is_empty());
    tick(&mut app, 0);
    let request = app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_requests.pop().unwrap();
    assert_eq!((request.description.as_str(), request.options.len()), ("ID_REPLAY_EDITOR_QUIT", 2));
    assert!(app.world().resource::<Replay>().active);
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_choices.push((QUIT_POPUP.into(), 0));
    tick(&mut app, 0);
    assert!(app.world().resource::<Replay>().active, "No stays in the replay");
    tick(&mut app, SELECT);
    tick(&mut app, 0);
    // Another system's answer stays queued for it (the teleporter's Yes was lost, session 2026-10-23).
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_choices
        .extend([(QUIT_POPUP.into(), 1), ("engine:teleporter".into(), 1)]);
    tick(&mut app, 0);
    assert!(!app.world().resource::<Replay>().active, "Yes leaves");
    assert_eq!(app.world().resource::<crate::popup_host::PopupMods>().engine_choices, [("engine:teleporter".to_owned(), 1)]);
}

#[test]
fn the_living_world_freezes_during_a_replay() {
    #[derive(Resource, Default)]
    struct Steps(u32);
    let mut app = App::new();
    app.init_resource::<Steps>()
        .add_systems(Update, (|mut s: ResMut<Steps>| s.0 += 1).run_if(world_live));
    app.update();
    assert_eq!(app.world().resource::<Steps>().0, 1, "no replay resource: runs");
    app.init_resource::<Replay>();
    for i in 0..400 {
        app.world_mut().resource_mut::<Replay>().record(pose(i as f32), 1. / 60., false);
    }
    app.update();
    assert_eq!(app.world().resource::<Steps>().0, 2);
    app.world_mut().resource_mut::<Replay>().enter();
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Steps>().0, 2, "frozen while the replay is open");
    app.world_mut().resource_mut::<Replay>().exit();
    app.update();
    assert_eq!(app.world().resource::<Steps>().0, 3);
}

#[test]
fn extended_replays_scale_the_window_and_a_mod_still_wins() {
    let retail = skate_core::replay_buffer::ReplayLimits::default();
    let mut replay = Replay::default();
    replay.set_user_length(Some(240));
    assert_eq!(replay.world.limits.max_frames, 240 * 60);
    assert_eq!(replay.world.limits.byte_budget, retail.byte_budget * 240 / 90);
    // A mod's limits win; clearing them goes back to the player's length, not retail.
    replay.set_limits("m", Some(600), None, None);
    assert_eq!(replay.world.limits.max_frames, 600);
    replay.set_user_length(Some(120));
    assert_eq!(replay.world.limits.max_frames, 600);
    replay.clear_owner("m");
    assert_eq!(replay.world.limits.max_frames, 120 * 60);
    replay.set_user_length(None);
    assert_eq!(replay.world.limits, retail);
}

#[test]
fn a_still_prop_added_between_replay_frames_still_plays_back() {
    use super::world::ReplayActor;
    let mut app = App::new();
    app.init_resource::<Replay>();
    super::world::plugin(&mut app);
    let step = |app: &mut App, x: f32| {
        app.world_mut().resource_mut::<Replay>().record(pose(x), 1. / 60., false);
        app.update();
    };
    step(&mut app, 0.);
    // Tagged on a render frame that brings no new replay frame (its `Added` tick is not recorded), then never moves.
    let bench = app.world_mut().spawn((ReplayActor { id: 11 }, Transform::from_xyz(3., 0., 0.), Visibility::Inherited)).id();
    app.update();
    step(&mut app, 1.);
    step(&mut app, 2.);
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(1.5 / 60.);
    }
    app.update();
    assert_eq!(app.world().get::<Visibility>(bench), Some(&Visibility::Inherited), "the still bench is drawn");
}

/// b453: a held prop's hand-space frames play back on a hidden copy after it is dropped, and the dropped prop's own
/// world-space track starts at the drop, so scrubbing back never shows the dropped model at a hand-space pose.
#[test]
fn a_dropped_hand_prop_keeps_its_held_frames_on_a_hidden_copy() {
    use super::world::{ReplayActor, ReplayGhost};
    use bevy::ecs::system::RunSystemOnce;
    let mut app = App::new();
    app.init_resource::<Replay>();
    super::world::plugin(&mut app);
    let step = |app: &mut App, x: f32| {
        app.world_mut().resource_mut::<Replay>().record(pose(x), 1. / 60., false);
        app.update();
    };
    let hand = app.world_mut().spawn((Transform::from_xyz(10., 0., 0.), Visibility::Inherited)).id();
    let held = Transform::from_xyz(1., 0., 0.);
    let prop = app.world_mut().spawn((ReplayActor { id: 21 }, SceneRoot::default(), held, Visibility::Inherited, ChildOf(hand))).id();
    step(&mut app, 0.);
    step(&mut app, 1.);
    // The drop: the copy is kept, then the model is detached into the world and re-tagged as a prop.
    app.world_mut().run_system_once(move |mut commands: Commands| super::keep_model(&mut commands, prop)).unwrap();
    app.world_mut().entity_mut(prop).remove::<ChildOf>().insert((Transform::from_xyz(11., 0., 0.), ReplayActor { id: 22 }));
    step(&mut app, 2.);
    step(&mut app, 3.);
    let ghost = app.world_mut().query_filtered::<Entity, With<ReplayGhost>>().single(app.world()).unwrap();
    assert_eq!(app.world().get::<ChildOf>(ghost).map(ChildOf::parent), Some(hand), "the copy stays in the hand's space");
    let seek = |app: &mut App, t: f64| {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(t);
        app.update();
    };
    seek(&mut app, 0.5 / 60.);
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Inherited), "held frames: the copy draws");
    assert_eq!(app.world().get::<Transform>(ghost).map(|t| t.translation), Some(held.translation));
    assert_eq!(app.world().get::<Visibility>(prop), Some(&Visibility::Hidden), "held frames: the dropped model is hidden");
    seek(&mut app, 3. / 60.);
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden), "after the drop the copy is hidden");
    assert_eq!(app.world().get::<Visibility>(prop), Some(&Visibility::Inherited));
    assert_eq!(app.world().get::<Transform>(prop).map(|t| t.translation), Some(Vec3::new(11., 0., 0.)));
}

#[test]
fn retail_screen_keys_help_panel_and_photo() {
    use crate::input::ControllerInput;
    use skate_core::input::xbox::XboxState;
    let mut app = App::new();
    app.init_resource::<Replay>()
        .init_resource::<ControllerInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
        .init_resource::<Time<Real>>()
        .init_resource::<crate::popup_host::PopupMods>()
        .insert_resource(RetailReplayUi(true, false))
        .add_message::<crate::ui_audio::FrontendSound>()
        .add_systems(Update, controls);
    let tick = |app: &mut App, buttons| {
        app.world_mut().resource_mut::<ControllerInput>().sample_raw_for_test(XboxState { buttons, triggers: [0; 2], left: [0; 2], right: [0; 2] });
        app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_secs_f64(1. / 60.));
        app.world_mut().run_schedule(Update);
    };
    for i in 0..400 {
        app.world_mut().resource_mut::<Replay>().record(pose(i as f32), 1. / 60., false);
    }
    app.world_mut().resource_mut::<Replay>().enter();
    // L3 hides and shows the panel.
    tick(&mut app, L3);
    tick(&mut app, 0);
    assert!(app.world().resource::<Replay>().panel_hidden);
    tick(&mut app, L3);
    tick(&mut app, 0);
    assert!(!app.world().resource::<Replay>().panel_hidden);
    // Start opens the help; A turns the page; B closes it without asking to quit.
    tick(&mut app, START);
    tick(&mut app, 0);
    assert!(app.world().resource::<Replay>().help_open);
    tick(&mut app, A);
    tick(&mut app, 0);
    assert_eq!(app.world().resource::<Replay>().help_turns, 1);
    tick(&mut app, B);
    tick(&mut app, 0);
    let replay = app.world().resource::<Replay>();
    assert!(!replay.help_open && replay.active && !replay.quit_pending);
    assert!(app.world().resource::<crate::popup_host::PopupMods>().engine_requests.is_empty());
    // X asks to save a photo; Yes requests the capture, the replay stays open and stopped.
    app.world_mut().resource_mut::<Replay>().playing = true;
    tick(&mut app, X);
    tick(&mut app, 0);
    let request = app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_requests.pop().unwrap();
    assert_eq!(request.description, "ID_REPLAY_EDITOR_POPUP_PHOTOSAVE");
    assert_eq!(app.world().resource::<Replay>().photo, Photo::Confirm);
    assert!(!app.world().resource::<Replay>().playing);
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_choices.push((PHOTO_POPUP.into(), 1));
    tick(&mut app, 0);
    assert_eq!(app.world().resource::<Replay>().photo, Photo::Capture);
}

#[test]
fn the_finished_menu_offers_save_to_pc_and_exit() {
    use crate::input::ControllerInput;
    use skate_core::input::xbox::XboxState;
    let mut app = App::new();
    app.init_resource::<Replay>()
        .init_resource::<ControllerInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
        .init_resource::<Time<Real>>()
        .init_resource::<crate::popup_host::PopupMods>()
        .init_resource::<export_run::ReplayExport>()
        .add_message::<crate::ui_audio::FrontendSound>()
        .add_systems(Update, controls);
    let tick = |app: &mut App, buttons| {
        app.world_mut().resource_mut::<ControllerInput>().sample_raw_for_test(XboxState { buttons, triggers: [0; 2], left: [0; 2], right: [0; 2] });
        app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_secs_f64(1. / 60.));
        app.world_mut().run_schedule(Update);
    };
    for i in 0..400 {
        app.world_mut().resource_mut::<Replay>().record(pose(i as f32), 1. / 60., false);
    }
    app.world_mut().resource_mut::<Replay>().enter();
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().stage_ready = true;
    // B in the trim mode (retail mode 2) is ignored: no Finished menu (only modes 1 / 5 / 6 raise it, b555).
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.editor = editor::Editor::open(0.0, 6.0, Default::default());
        replay.editor.as_mut().unwrap().session.mode = skate_core::replay_edit::session::EditorMode::Trim;
    }
    tick(&mut app, B);
    tick(&mut app, 0);
    assert!(app.world().resource::<crate::popup_host::PopupMods>().engine_requests.is_empty());
    app.world_mut().resource_mut::<Replay>().editor = None;
    tick(&mut app, B);
    tick(&mut app, 0);
    let request = app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_requests.pop().unwrap();
    assert_eq!(request.options, vec![export_run::SAVE_TO_PC.to_owned(), "ID_REPLAYEDITOR_EXIT".to_owned()]);
    // Cancel stays; Exit leaves.
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_choices.push((export_run::FINISHED_POPUP.into(), skate_core::fe_popup::CHOICE_CANCEL));
    tick(&mut app, 0);
    assert!(app.world().resource::<Replay>().active);
    // Save asks first (retail b387 row 2: Save Replay? No / Yes); No goes back to the editor.
    tick(&mut app, B);
    tick(&mut app, 0);
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_requests.clear();
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_choices.push((export_run::FINISHED_POPUP.into(), 0));
    tick(&mut app, 0);
    let confirm = app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_requests.pop().unwrap();
    assert_eq!((confirm.callback.as_str(), confirm.description.as_str()), (export_run::SAVE_CONFIRM_POPUP, "ID_REPLAY_EDITOR_POPUP_REPLAYSAVE"));
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_choices.push((export_run::SAVE_CONFIRM_POPUP.into(), 0));
    tick(&mut app, 0);
    assert!(app.world().resource::<Replay>().active && !app.world().resource::<Replay>().quit_pending);
    assert!(!app.world().resource::<export_run::ReplayExport>().busy());
    tick(&mut app, B);
    tick(&mut app, 0);
    app.world_mut().resource_mut::<crate::popup_host::PopupMods>().engine_choices.push((export_run::FINISHED_POPUP.into(), 1));
    tick(&mut app, 0);
    assert!(!app.world().resource::<Replay>().active);
}

/// The film pack (its look screen): X opens its Finished menu (Save, photo, replay mode, resets), the replay mode
/// popup toggles and stays up, a reset's No reopens the menu, B goes straight to the quit warning, and closing the
/// replay puts the replay mode back (b387 / b557).
#[test]
fn the_film_pack_finished_menu_works_as_retail() {
    use crate::input::ControllerInput;
    use crate::popup_host::PopupMods;
    use skate_core::input::xbox::XboxState;
    let mut app = App::new();
    app.init_resource::<Replay>()
        .init_resource::<ControllerInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
        .init_resource::<Time<Real>>()
        .init_resource::<PopupMods>()
        .init_resource::<export_run::ReplayExport>()
        .insert_resource(ReplayLookScreen(true))
        .add_message::<crate::ui_audio::FrontendSound>()
        .add_systems(Update, controls);
    let tick = |app: &mut App, buttons| {
        app.world_mut().resource_mut::<ControllerInput>().sample_raw_for_test(XboxState { buttons, triggers: [0; 2], left: [0; 2], right: [0; 2] });
        app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_secs_f64(1. / 60.));
        app.world_mut().run_schedule(Update);
    };
    let pop = |app: &mut App| app.world_mut().resource_mut::<PopupMods>().engine_requests.pop().unwrap();
    let choose = |app: &mut App, callback: &str, choice: i32| {
        app.world_mut().resource_mut::<PopupMods>().engine_choices.push((callback.into(), choice));
        tick(app, 0);
    };
    for i in 0..400 {
        app.world_mut().resource_mut::<Replay>().record(pose(i as f32), 1. / 60., false);
    }
    app.world_mut().resource_mut::<Replay>().enter();
    app.world_mut().resource_mut::<PopupMods>().stage_ready = true;
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.editor = editor::Editor::open(0.0, 6.0, Default::default());
        replay.editor.as_mut().unwrap().session.mode = skate_core::replay_edit::session::EditorMode::Timeline;
    }
    tick(&mut app, X);
    tick(&mut app, 0);
    let menu = pop(&mut app);
    assert_eq!(menu.callback, FILM_MENU_POPUP);
    assert_eq!(menu.options, film_menu_rows(true).into_iter().map(String::from).collect::<Vec<_>>());
    // The open menu owns the pad (USER-ASKS 342): A, the D-pad and RB do nothing underneath it.
    let (cursor, playing) = { let r = app.world().resource::<Replay>(); (r.cursor, r.playing) };
    for buttons in [A, 0, DPAD_DOWN, 0, RB, 0] {
        tick(&mut app, buttons);
    }
    {
        let r = app.world().resource::<Replay>();
        assert_eq!((r.cursor, r.playing), (cursor, playing));
        assert_eq!(r.editor.as_ref().unwrap().session.mode, skate_core::replay_edit::session::EditorMode::Timeline);
    }
    // Replay mode: a toggle stays up; cancel closes.
    choose(&mut app, FILM_MENU_POPUP, 2);
    assert_eq!(pop(&mut app).callback, REPLAY_MODE_POPUP);
    choose(&mut app, REPLAY_MODE_POPUP, 0);
    assert_eq!(pop(&mut app).callback, REPLAY_MODE_POPUP);
    assert!(app.world().resource::<Replay>().view.greenscreen);
    choose(&mut app, REPLAY_MODE_POPUP, skate_core::fe_popup::CHOICE_CANCEL);
    assert!(app.world().resource::<PopupMods>().engine_requests.is_empty());
    // Reset Timeline: No reopens the Finished menu.
    tick(&mut app, X);
    tick(&mut app, 0);
    pop(&mut app);
    choose(&mut app, FILM_MENU_POPUP, 4);
    assert_eq!(pop(&mut app).description, "ID_REPLAY_EDITOR_RESET_TIMELINE_CONFRIM");
    choose(&mut app, RESET_POPUP, 0);
    assert_eq!(pop(&mut app).callback, FILM_MENU_POPUP);
    choose(&mut app, FILM_MENU_POPUP, skate_core::fe_popup::CHOICE_CANCEL);
    // B: the quit warning, not the Finished menu; Yes closes and the replay mode is back to the defaults.
    tick(&mut app, B);
    tick(&mut app, 0);
    assert_eq!(pop(&mut app).callback, QUIT_POPUP);
    choose(&mut app, QUIT_POPUP, 1);
    let replay = app.world().resource::<Replay>();
    assert!(!replay.active && replay.view == ReplayViewMode::default());
}

/// Characters follow "skaters visible"; everything else hides under greenscreen (`827F9D40`, b557).
#[test]
fn replay_mode_flags_pick_what_draws() {
    let mut view = ReplayViewMode::default();
    assert!((0..=5).all(|kind| view.shows(kind)));
    view.toggle(0);
    assert!(view.shows(0) && view.shows(1) && !view.shows(2) && !view.shows(3) && !view.shows(5));
    view.toggle(2);
    assert!(!view.shows(0) && !view.shows(1));
    view.toggle(1);
    assert!(!view.player);
}

/// USER-ASKS 350 (laptop session game-20261025-152135): an Advanced Follow key pitched down at the logged spot on
/// DownTown must stay above the ground; the edit-time constraint holds it (b593). Prints the eye against the ground.
#[test]
#[ignore = "requires an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<installation>/maps/DownTown.skate)"]
fn a_follow_key_pitched_down_on_downtown_stays_above_the_ground() {
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map = skate_data::skate_map::SkateMap::load(std::path::Path::new(&std::env::var_os("SKATE3_MAP").unwrap())).unwrap();
    let physics = crate::physics::GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    let world = physics.world();
    let ground = |p: Vec3| camera_collision::sweep(world, p + Vec3::Y * 3.0, p - Vec3::Y * 10.0, 0.0).map(|h| p.y + 3.0 - 13.0 * h.fraction);
    let root = Transform::from_xyz(-259.95566, 40.306458, 50.564365);
    let camera = Transform::from_xyz(-259.16074, 40.80301, 48.966225).looking_at(root.translation, Vec3::Y);
    let anchors = camera_key::Anchors::new(camera, root, camera_key::FollowEye::Camera);
    println!("ground under root {:?}", ground(root.translation));
    let mut e = editor::Editor::open(0.0, 20.0, skate_core::replay_edit::EditLimits::default()).unwrap();
    assert!(e.pick_camera(4, 10.0, &camera, &anchors).is_some());
    let collision = camera_collision::CameraCollision::default();
    let mut lowest = f32::MAX;
    for (frames, turn) in [(90, Vec2::new(-1.0, 0.0)), (120, Vec2::new(-0.5, -0.8)), (60, Vec2::new(0.0, -1.0))] {
        for _ in 0..frames {
            e.edit_key(camera_key::CameraInput { turn, ..default() }, Some(&anchors), 1.0 / 60.0);
            e.constrain_editing(&anchors, &collision, |from, to, radius| camera_collision::sweep(world, from, to, radius));
            let eye = e.pose(10.0, &anchors).0.unwrap().translation;
            if let Some(g) = ground(eye) {
                lowest = lowest.min(eye.y - g);
            }
        }
    }
    let eye = e.pose(10.0, &anchors).0.unwrap().translation;
    println!("eye {eye} ground {:?} lowest clearance {lowest}", ground(eye));
    assert!(lowest > -0.01, "the eye went {lowest} m under the ground");
}

#[test]
/// Retail's replay camera never clips through the ground or ramps (user 2026-10-29): every play mode is constrained.
fn keyed_cameras_are_constrained_whenever_the_clip_plays() {
    use skate_core::replay_edit::session::EditorMode;
    let root = Transform::from_xyz(0.0, 1.0, 0.0);
    let camera = Transform::from_xyz(0.0, 3.0, 8.0).looking_at(root.translation, Vec3::Y);
    let anchors = camera_key::Anchors::new(camera, root, camera_key::FollowEye::Camera);
    let mut replay = Replay::default();
    let mut e = editor::Editor::open(0.0, 20.0, skate_core::replay_edit::EditLimits::default()).unwrap();
    assert!(e.pick_camera(4, 10.0, &camera, &anchors).is_some());
    replay.editor = Some(e);
    replay.cursor = 10.0;
    let wall = |from: Vec3, to: Vec3, _: f32| Some(camera_collision::Hit { fraction: 0.5, normal: (from - to).normalize() });
    let eye = |replay: &mut Replay, mode| {
        replay.editor.as_mut().unwrap().session.mode = mode;
        replay.guarded_editor_camera(&camera, &root, &mut CameraGuard::default(), wall).0.unwrap().translation
    };
    let stored = replay.editor_camera(&camera, &root).0.unwrap().translation;
    for mode in [EditorMode::Instant, EditorMode::Playback, EditorMode::Timeline, EditorMode::Trim] {
        assert_ne!(eye(&mut replay, mode), stored, "{mode:?}: kept out of the wall");
    }
    replay.set_camera("m", None, None, None, None, None, Some(false));
    assert_eq!(eye(&mut replay, EditorMode::Playback), stored, "a mod's playback_collide = false plays the stored key");
    assert_ne!(eye(&mut replay, EditorMode::Instant), stored, "the instant view stays constrained");
}
