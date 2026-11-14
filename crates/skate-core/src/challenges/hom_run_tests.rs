use super::*;

const BOUNDARY: u64 = 0xB0;

fn run_with(params: Params) -> HomRun {
    let mut run = HomRun::new(params);
    run.start();
    run.handle(Event::HudLoaded);
    ticks(&mut run, 60);
    run.handle(Event::TeleportFinished);
    ticks(&mut run, 61 + 91 + 181);
    assert_eq!(run.state(), State::RunChallenge);
    run
}

fn running() -> HomRun {
    run_with(Params { boundary: Some(BOUNDARY), ..Params::retail() })
}

fn ticks(run: &mut HomRun, n: usize) -> Vec<Command> {
    (0..n).flat_map(|_| run.tick()).collect()
}

fn finish(run: &mut HomRun) -> Vec<Command> {
    let mut out = ticks(run, 400);
    out.extend(run.handle(Event::HighlightReelComplete));
    out.extend(run.handle(Event::Continue));
    out
}

#[test]
fn the_setup_reaches_the_run_through_the_countdown() {
    let mut run = HomRun::new(Params::retail());
    let out = run.start();
    assert!(out.contains(&Command::PinPlayer(true)) && out.contains(&Command::CreateHud));
    let out = run.handle(Event::HudLoaded);
    assert!(out.contains(&Command::HomHudInChallenge(true)), "SetHOMHudGroup(Name) at HudLoaded");
    assert!(out.contains(&Command::ShowHud(HudComponent::TrickDisplay, false)));
    ticks(&mut run, 60);
    assert_eq!(run.state(), State::SetupChallengeCommon);
    run.handle(Event::TeleportFinished);
    let out = ticks(&mut run, 61);
    assert!(out.contains(&Command::SetSessionMarker), "the start is the reset marker");
    let out = ticks(&mut run, 91);
    assert!(out.contains(&Command::StartCountdownHud { ticks: 180 }), "1.5 s delay, then the 3 s countdown");
    let out = ticks(&mut run, 181);
    assert_eq!(run.state(), State::RunChallenge);
    assert!(out.contains(&Command::Evaluation(true)) && out.contains(&Command::PinPlayer(false)));
    assert!(!out.iter().any(|c| matches!(c, Command::StartTimerHud { .. })), "no TimeLimit: a count-up, no clock");
}

#[test]
fn a_bail_of_an_unfinished_run_goes_back_to_the_start_unless_a_marker_was_used() {
    let mut run = running();
    run.handle(Event::Wipeout);
    let out = run.handle(Event::WipeoutOver { score: 1200 });
    assert!(out.contains(&Command::RepositionToStart), "wipeoutover@149-180: single player restarts");
    assert!(out.contains(&Command::HighlightFromBail));
    run.handle(Event::Wipeout);
    run.handle(Event::UseMarker);
    let out = run.handle(Event::WipeoutOver { score: 500 });
    assert!(!out.contains(&Command::RepositionToStart), "usemarker@15-34 cancels the teleport");
    assert_eq!(run.competitor().highest_score, 1200, "the best bail is kept (strictly greater)");
    assert!(!out.contains(&Command::HighlightFromBail), "a lower bail keeps the highlight");
}

#[test]
fn a_mod_can_switch_the_restart_on_bail_off() {
    let mut run = run_with(Params { restart_on_wipeout: false, ..Params::retail() });
    run.handle(Event::Wipeout);
    assert!(!run.handle(Event::WipeoutOver { score: 10 }).contains(&Command::RepositionToStart));
}

#[test]
fn the_owned_group_completes_after_the_pending_window_and_shows_owned_it() {
    let mut run = running();
    ticks(&mut run, 120);
    run.handle(Event::AllObjectivesComplete { killed: false });
    assert!(run.competitor().complete && run.competitor().result == Completion::Owned);
    assert!((run.competitor().time - 2.0).abs() < 0.05, "{}", run.competitor().time);
    let out = ticks(&mut run, 31);
    assert!(out.contains(&Command::ShowHud(HudComponent::OwnedIt, true)), "the 0.5 s owned pending");
    let out = finish(&mut run);
    assert!(out.contains(&Command::Complete(Completion::Owned)));
    assert!(out.contains(&Command::HomHudInChallenge(false)), "SetHOMHudGroup(\"main\") at cleanup");
    assert_eq!(run.state(), State::Complete);
}

#[test]
fn a_killed_completion_inside_the_owned_window_wins_killed() {
    let mut run = running();
    run.handle(Event::AllObjectivesComplete { killed: false });
    ticks(&mut run, 10);
    let out = run.handle(Event::AllObjectivesComplete { killed: true });
    assert!(out.contains(&Command::ShowHud(HudComponent::KilledIt, true)));
    let out = ticks(&mut run, 40);
    assert!(!out.contains(&Command::ShowHud(HudComponent::OwnedIt, true)), "the pending owned banner is dropped");
    assert!(finish(&mut run).contains(&Command::Complete(Completion::Killed)));
}

#[test]
fn a_replay_with_a_killed_group_counts_only_the_killed_group() {
    let mut run = run_with(Params { has_killed_group: true, stored: Completion::Owned, ..Params::retail() });
    run.handle(Event::AllObjectivesComplete { killed: false });
    assert!(!run.competitor().complete, "ShowKilledItObj: owned completions are ignored");
    run.handle(Event::AllObjectivesComplete { killed: true });
    assert_eq!(run.competitor().result, Completion::Killed);
}

#[test]
fn scoring_low_on_the_owned_group_ends_the_run_as_a_loss() {
    let mut run = running();
    let out = run.handle(Event::DidNotGetEnoughPoints { killed: false });
    assert!(out.contains(&Command::ScoredLow));
    assert!(!out.iter().any(|c| matches!(c, Command::ShowHud(HudComponent::OwnedIt | HudComponent::KilledIt, true))));
    assert!(finish(&mut run).contains(&Command::Complete(Completion::Abandoned)));
    let mut run = running();
    assert!(run.handle(Event::DidNotGetEnoughPoints { killed: true }).is_empty(), "a killed group miss only prints");
}

#[test]
fn a_timed_row_runs_out_and_a_bailing_skater_goes_to_the_start() {
    let mut run = run_with(Params { time_limit: Some(2.0), ..Params::retail() });
    run.handle(Event::Wipeout);
    let out = ticks(&mut run, 121);
    assert!(out.contains(&Command::PinPlayer(true)) && out.contains(&Command::RepositionToStart));
    assert_eq!(run.competitor().finish, Some(Finish::TimeUp));
    ticks(&mut run, 61);
    assert_eq!(run.state(), State::Highlight);
    let out = finish(&mut run);
    assert!(out.contains(&Command::Complete(Completion::Abandoned)));
}

#[test]
fn single_player_can_switch_the_time_limit_off() {
    let p = Params { time_limit: Some(90.0), disable_time_limit_for_single_player: true, ..Params::retail() };
    assert!(!p.time_limited());
    assert!(Params { single_player: false, ..p.clone() }.time_limited());
    let mut run = run_with(p);
    ticks(&mut run, 60 * 100);
    assert_eq!(run.state(), State::RunChallenge, "counts up with no limit");
    assert!(run.elapsed_ticks() >= 6000);
}

#[test]
fn leaving_the_boundary_repositions_and_the_run_goes_on() {
    let mut run = running();
    let out = run.handle(Event::TriggerExit { volume: BOUNDARY });
    assert!(out.contains(&Command::ReturnToChallengeArea));
    let out = ticks(&mut run, 301);
    assert!(out.contains(&Command::RepositionToStart));
    assert_eq!(run.state(), State::RunChallenge, "out of bounds never fails a Hall of Meat run");
    run.handle(Event::TriggerExit { volume: BOUNDARY });
    run.handle(Event::TriggerEnter { volume: BOUNDARY });
    assert!(!ticks(&mut run, 301).contains(&Command::RepositionToStart), "coming back cancels it");
}

#[test]
fn retry_from_the_results_starts_a_fresh_attempt() {
    let mut run = running();
    run.handle(Event::Wipeout);
    run.handle(Event::WipeoutOver { score: 900 });
    run.handle(Event::DidNotGetEnoughPoints { killed: false });
    ticks(&mut run, 400);
    run.handle(Event::HighlightReelComplete);
    assert_eq!(run.state(), State::Results);
    run.handle(Event::Retry);
    assert_eq!(run.state(), State::SetupChallengeCommon);
    run.handle(Event::TeleportFinished);
    ticks(&mut run, 61);
    assert_eq!((run.competitor().highest_score, run.competitor().complete), (0, false));
}

#[test]
fn abandon_ends_as_abandoned_from_anywhere() {
    let mut run = running();
    run.handle(Event::AllObjectivesComplete { killed: false });
    let out = run.handle(Event::Abandon);
    assert!(out.contains(&Command::Complete(Completion::Abandoned)));
    assert!(run.finished());
}

#[test]
fn points_only_ever_raise_the_dpo_bar() {
    let mut run = running();
    assert!(run.handle(Event::ObjectivePointsAdded { points: 300 }).contains(&Command::Dpo { score: 300 }));
    assert!(run.handle(Event::ObjectivePointsAdded { points: 200 }).is_empty());
}
