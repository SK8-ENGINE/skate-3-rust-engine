use super::*;

const END: u64 = 0xE0D;
const BOUNDARY: u64 = 0xB0;

fn ftl() -> FilmRun {
    FilmRun::new(Params { ftl_end_volume: Some(END), ..Params::retail(Kind::Follow) })
}

fn film() -> FilmRun {
    FilmRun::new(Params { boundary: Some(BOUNDARY), ..Params::retail(Kind::Film) })
}

/// Ticks until the state changes (at most `limit`), all commands on the way.
fn tick_until(run: &mut FilmRun, state: State, limit: u32) -> Vec<Command> {
    let mut out = Vec::new();
    for _ in 0..limit {
        out.extend(run.tick());
        if run.state() == state {
            return out;
        }
    }
    panic!("not in {state:?} after {limit} ticks (in {:?})", run.state());
}

/// start -> hud -> assets -> AI -> teleport -> countdown done: the run is on.
fn to_running(run: &mut FilmRun) -> Vec<Command> {
    let mut out = run.start();
    assert_eq!(run.state(), State::PrefilmDisplay);
    out.extend(run.handle(Event::HudLoaded));
    assert_eq!(run.state(), State::WaitForAssetLoad, "no intro events: straight to setup");
    out.extend(run.handle(Event::AssetsLoaded));
    if run.state() == State::CheckAi {
        out.extend(run.handle(Event::AiExist(true)));
    }
    assert_eq!(run.state(), State::Countdown);
    out.extend(run.handle(Event::TeleportFinished));
    out.extend(tick_until(run, State::ChallengeRunning, 200));
    out
}

#[test]
fn follow_the_leader_wins_when_the_owned_objective_completes_and_waits_half_a_second_for_a_kill() {
    let mut run = ftl();
    let out = to_running(&mut run);
    for c in [Command::PinPlayer(true), Command::SpawnAi, Command::PinAi(true), Command::RepositionPlayer, Command::ShowHud(HudComponent::DistanceMeter, true),
              Command::DistanceUpdates(true), Command::PinPlayer(false), Command::PinAi(false), Command::StartAiPaths, Command::ActivateStartObjectives] {
        assert!(out.contains(&c), "{c:?} missing");
    }
    assert!(!out.iter().any(|c| matches!(c, Command::StartTimerHud { .. })), "follow the leader has no film timer");
    // The leader reaches the end: it stops, the meter hides, a 10 s window opens.
    let out = run.handle(Event::TriggerEnter { volume: END, competitor: 1 });
    assert!(out.contains(&Command::PinLeader) && out.contains(&Command::StartTimerHud { ticks: 600 }));
    assert_eq!(run.ftl_ticks(), Some(600));
    // The player reaches the end volume: the owned objective completes; the owned banner waits 0.5 s.
    run.handle(Event::AllObjectivesComplete { killed: false });
    assert_eq!(run.result(), Flow::Win);
    let out = tick_until(&mut run, State::KilledIt, 40);
    assert!(out.contains(&Command::ShowHud(HudComponent::OwnedIt, true)));
    tick_until(&mut run, State::HighlightReel, 200);
    run.handle(Event::HighlightReelComplete);
    assert_eq!(run.state(), State::EndOptions);
    let out = run.handle(Event::Continue);
    assert_eq!(out.last(), Some(&Command::Complete(Completion::Owned)));
}

#[test]
fn follow_the_leader_ends_abandoned_when_the_ten_second_window_runs_out_and_a_bail_changes_nothing() {
    let mut run = ftl();
    to_running(&mut run);
    assert!(run.handle(Event::Wipeout).is_empty(), "AllowPlayerBail: the run goes on");
    assert_eq!(run.state(), State::ChallengeRunning);
    run.handle(Event::TriggerEnter { volume: END, competitor: 0 });
    assert_eq!(run.ftl_ticks(), None, "only the leader's entry opens the window");
    run.handle(Event::TriggerEnter { volume: END, competitor: 1 });
    tick_until(&mut run, State::KilledIt, 602);
    assert_eq!(run.result(), Flow::Lose);
    tick_until(&mut run, State::EndOptions, 100);
    assert_eq!(run.handle(Event::Continue).last(), Some(&Command::Complete(Completion::Abandoned)));
}

#[test]
fn follow_the_leader_too_far_behind_fails_the_objective_and_ends_the_run() {
    let mut run = ftl();
    to_running(&mut run);
    let out = run.handle(Event::ObjectiveFailed);
    assert!(out.contains(&Command::FailAllObjectives) && out.contains(&Command::CentralMessage(Message::ObjectiveFailure)));
    assert_eq!(run.state(), State::KilledIt, "cleanup ran and went on");
    let out = tick_until(&mut run, State::EndOptions, 100);
    assert!(out.contains(&Command::ShowHud(HudComponent::KilledIt, false)), "no reel on a loss");
    let out = run.handle(Event::Retry);
    assert_eq!(run.state(), State::WaitForAssetLoad, "restarted -> setup -> asset wait");
    assert!(out.contains(&Command::ResetObjectives));
    run.handle(Event::AssetsLoaded);
    assert_eq!(run.state(), State::Countdown, "the leader exists already: no new spawn on a restart");
}

#[test]
fn an_offline_film_has_no_time_limit_but_keeps_the_low_time_speech() {
    let mut run = film();
    let out = to_running(&mut run);
    assert!(!out.iter().any(|c| matches!(c, Command::StartTimerHud { .. })), "no film clock offline (b999 §0.1)");
    assert_eq!(run.film_ticks(), None);
    let out: Vec<Command> = (0..181 * 60).flat_map(|_| run.tick()).collect();
    assert!(out.contains(&Command::Speech(Speech::TimeLimitLow)), "the warning timer still runs (b999 §0.2)");
    assert!(!out.contains(&Command::CentralMessage(Message::TimeUp)));
    assert_eq!(run.state(), State::ChallengeRunning, "past 180 s the film goes on");
    let mut anywhere = FilmRun::new(Params::retail(Kind::AnywhereFilm));
    let out = to_running(&mut anywhere);
    let out: Vec<Command> = out.into_iter().chain((0..31 * 60).flat_map(|_| anywhere.tick())).collect();
    assert!(!out.contains(&Command::Speech(Speech::TimeLimitLow)) && anywhere.state() == State::ChallengeRunning,
        "anywhere films: no FilmTimerLowLimit, no warning, no time limit (b1003 §4)");
}

#[test]
fn a_film_in_simul_play_runs_out_of_time_and_a_wipeout_rearms_instead_of_ending() {
    let mut run = FilmRun::new(Params { boundary: Some(BOUNDARY), simul_play: true, ..Params::retail(Kind::Film) });
    let out = to_running(&mut run);
    assert!(out.contains(&Command::StartTimerHud { ticks: 180 * 60 }));
    assert_eq!(run.film_ticks(), Some(180 * 60));
    let out = run.handle(Event::Wipeout);
    assert!(out.contains(&Command::FailAllObjectives) && out.contains(&Command::Speech(Speech::Wipeout)));
    let out: Vec<Command> = (0..95).flat_map(|_| run.tick()).collect();
    assert_eq!(run.state(), State::ChallengeRunning);
    assert!(out.contains(&Command::RepositionPlayer) && out.contains(&Command::ResetObjectives), "1.5 s later: back to the start, objectives re-armed");
    // The low-time speech at 5 s left, then time up.
    let out = tick_until(&mut run, State::KilledIt, 180 * 60 + 2);
    assert!(out.contains(&Command::Speech(Speech::TimeLimitLow)) && out.contains(&Command::CentralMessage(Message::TimeUp)));
}

#[test]
fn leaving_a_films_boundary_counts_down_and_coming_back_cancels_it() {
    let mut run = film();
    to_running(&mut run);
    let out = run.handle(Event::TriggerExit { volume: BOUNDARY });
    assert!(out.contains(&Command::CentralMessage(Message::ReturnToArea)));
    run.handle(Event::TriggerEnter { volume: BOUNDARY, competitor: 0 });
    for _ in 0..400 {
        assert!(!run.tick().contains(&Command::CentralMessage(Message::OutOfBounds)));
    }
    run.handle(Event::TriggerExit { volume: BOUNDARY });
    let out: Vec<Command> = (0..303).flat_map(|_| run.tick()).collect();
    assert!(out.contains(&Command::CentralMessage(Message::OutOfBounds)) && out.contains(&Command::RepositionPlayer), "a film repairs: back to the start");
    assert_eq!(out.iter().filter(|c| **c == Command::RepositionPlayer).count(), 1,
        "once: the selector runs before the out-of-bounds flag is set (b999 §2)");
    assert_eq!(run.state(), State::ChallengeRunning);
}

#[test]
fn a_kill_beats_a_pending_owned_and_the_retail_values_are_data() {
    let mut run = film();
    to_running(&mut run);
    run.handle(Event::AllObjectivesComplete { killed: false });
    let out = run.handle(Event::AllObjectivesComplete { killed: true });
    assert!(out.contains(&Command::KilledItEffect(true)));
    assert_eq!((run.state(), run.result()), (State::KilledIt, Flow::Kill));
    let p = Params::retail(Kind::AnywhereFilm);
    assert_eq!((p.total_film_time, p.prefilm_time, p.rules.no_boundary, p.allow_objective_failure), (30.0, 1.0, true, true));
    assert_eq!((Params::retail(Kind::Follow).ftl_window, Params::retail(Kind::Follow).competitors), (10.0, 1));
}
