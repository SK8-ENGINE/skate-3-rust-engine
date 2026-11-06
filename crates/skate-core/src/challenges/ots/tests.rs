use super::*;

const BOUNDARY: u64 = 0x0008_B0CD_03E3_8706;

fn params() -> Params {
    Params {
        boundary: BOUNDARY,
        owned_points: 1000,
        killed_points: 1500,
        out_of_bounds_time: 10.0,
        owned_display_time: 3.0,
        killed_display_time: 5.0,
    }
}

fn running(personal_best: i32) -> Ots {
    let mut ots = Ots::new(params(), personal_best, true);
    let out = ots.start();
    assert_eq!(ots.state(), State::SetupHud);
    assert!(out.contains(&Command::CreateHud));
    let out = ots.handle(Event::HudLoaded);
    assert_eq!(ots.state(), State::ChallengeRunning);
    assert!(out.contains(&Command::Teleport));
    assert!(out.ends_with(&[Command::PauseMenu(true), Command::ResetObjectives, Command::Evaluation(true)]));
    ots
}

fn ticks(ots: &mut Ots, seconds: f32) -> Vec<Command> {
    (0..Timers::ticks(seconds)).flat_map(|_| ots.tick()).collect()
}

#[test]
fn first_target_is_the_owned_score() {
    let mut ots = running(0);
    let out = ots.handle(Event::NotEnoughPoints { points: 10 });
    assert!(out.contains(&Command::UpdateHud { score: 10, target: 1000, personal_best: 0, text: TargetText::BeatOwnSpot }));
}

#[test]
fn owning_it_completes_as_owned_after_the_reel() {
    let mut ots = running(0);
    let out = ots.handle(Event::AllObjectivesComplete { points: 1200 });
    assert!(out.contains(&Command::ShowHud(HudComponent::OwnedIt, true)));
    assert_eq!(ots.state(), State::HighlightReel);
    assert!(ticks(&mut ots, 2.9).is_empty() || ots.state() == State::HighlightReel);
    let out = ticks(&mut ots, 0.2);
    assert!(out.contains(&Command::PlayHighlightReel));
    let out = ots.handle(Event::HighlightReelComplete);
    assert!(out.contains(&Command::SetHighScore(1200)));
    assert_eq!(out.last(), Some(&Command::Complete(Completion::Owned)));
    assert!(ots.finished());
}

#[test]
fn killing_it_completes_as_killed_and_reads_the_display_times_crossed() {
    let mut ots = running(0);
    ots.handle(Event::AllObjectivesComplete { points: 1500 });
    // The treatment timer uses OwnedItHUDDisplayTime (3 s) for killed it, the reel wait KilledItHUDDisplayTime (5 s).
    let hidden = ticks(&mut ots, 3.0);
    assert!(hidden.contains(&Command::ShowHud(HudComponent::KilledIt, false)));
    assert!(!hidden.contains(&Command::PlayHighlightReel));
    assert!(ticks(&mut ots, 2.0).contains(&Command::PlayHighlightReel));
    let out = ots.handle(Event::HighlightReelComplete);
    assert_eq!(out.last(), Some(&Command::Complete(Completion::Killed)));
}

#[test]
fn a_win_at_the_same_tier_loops_back_instead_of_completing() {
    let mut ots = running(1600);
    ots.handle(Event::AllObjectivesComplete { points: 1700 });
    ticks(&mut ots, 5.0);
    let out = ots.handle(Event::HighlightReelComplete);
    assert!(!out.iter().any(|c| matches!(c, Command::Complete(_))));
    assert_eq!(ots.state(), State::ChallengeRunning);
    assert!(out.contains(&Command::Teleport));
    assert_eq!(ots.personal_best(), 1700);
}

#[test]
fn out_of_bounds_expiry_abandons() {
    let mut ots = running(0);
    let out = ots.handle(Event::TriggerExit { volume: BOUNDARY });
    assert!(out.contains(&Command::ReturnToAreaMessage));
    assert!(out.contains(&Command::StartTimerHud { ticks: 600 }));
    let out = ticks(&mut ots, 10.0);
    assert_eq!(out.last(), Some(&Command::Complete(Completion::Abandoned)));
}

#[test]
fn coming_back_in_time_cancels_the_countdown() {
    let mut ots = running(0);
    ots.handle(Event::TriggerExit { volume: BOUNDARY });
    ticks(&mut ots, 5.0);
    let out = ots.handle(Event::TriggerEnter { volume: BOUNDARY });
    assert!(out.contains(&Command::Evaluation(true)));
    assert!(ticks(&mut ots, 10.0).is_empty());
    assert_eq!(ots.state(), State::ChallengeRunning);
}

#[test]
fn other_volumes_are_ignored() {
    let mut ots = running(0);
    assert!(ots.handle(Event::TriggerExit { volume: 1 }).is_empty());
}

#[test]
fn not_enough_points_freezes_scoring_half_a_second() {
    let mut ots = running(0);
    let out = ots.handle(Event::NotEnoughPoints { points: 400 });
    assert!(out.contains(&Command::SetHighScore(400)));
    assert!(out.ends_with(&[Command::ResetObjectives, Command::Evaluation(false)]));
    assert!(ticks(&mut ots, 0.49).is_empty());
    assert!(ticks(&mut ots, 0.02).contains(&Command::Evaluation(true)));
    assert_eq!(ots.state(), State::ChallengeRunning);
}

#[test]
fn a_bail_pauses_and_resets() {
    let mut ots = running(0);
    assert_eq!(ots.handle(Event::Wipeout), vec![Command::Evaluation(false)]);
    assert!(ots.handle(Event::AllObjectivesComplete { points: 2000 }).is_empty());
    assert_eq!(ots.handle(Event::WipeoutOver), vec![Command::ResetObjectives, Command::Evaluation(true)]);
}

#[test]
fn quitting_abandons() {
    let mut ots = running(0);
    let out = ots.handle(Event::Abandon);
    assert_eq!(out.last(), Some(&Command::Complete(Completion::Abandoned)));
    assert!(ots.handle(Event::RestartChallenge).is_empty());
}

#[test]
fn the_intro_is_marked_seen_once() {
    let mut ots = Ots::new(params(), 0, false);
    assert!(ots.start().contains(&Command::MarkIntroSeen));
    ots.handle(Event::HudLoaded);
    let out = ots.handle(Event::RestartChallenge);
    assert!(!out.contains(&Command::MarkIntroSeen));
    assert!(out.contains(&Command::Teleport));
}
