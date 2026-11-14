//! The start's loading phase behind the splash: hidden window, holds via [`StartupLoading::hold`] / `release`.
//! Details: docs/hails-additions/08-setup-performance.md#startup-loading-phase
use std::sync::OnceLock;
use std::time::Instant;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// The start of `main`, for the `STARTUP_STEP` timeline.
static LAUNCH: OnceLock<Instant> = OnceLock::new();

pub(crate) fn start_clock() {
    LAUNCH.get_or_init(Instant::now);
}

/// Set once the game is shown: later marks (a map change's audio restart) print nothing.
static SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One timeline line while the start runs (any thread): milliseconds since `main` started, then the step.
pub(crate) fn mark(step: &str) {
    if !SHOWN.load(std::sync::atomic::Ordering::Relaxed) {
        eprintln!("STARTUP_STEP {} ms {step}", LAUNCH.get_or_init(Instant::now).elapsed().as_millis());
    }
}

/// A start-up stage: the crash report's `REPORT_META stage=` line plus the timeline line.
pub(crate) fn stage_meta(stage: &str) {
    eprintln!("REPORT_META stage={stage}");
    mark(stage);
}

/// What a loading step reads right now, shown on the splash (any thread). A no-op once the splash is gone, so bank
/// loads during play pay one atomic load.
pub(crate) fn detail(text: impl FnOnce() -> String) {
    if crate::splash::active() {
        crate::splash::detail(text());
    }
}

/// The holds keeping the loading phase going, in the order they were taken (the first one is the splash's step).
#[derive(Resource)]
pub(crate) struct StartupLoading {
    holds: Vec<(String, String)>,
    started: Instant,
    done: bool,
    first_frame: bool,
    /// Frames the splash stays up after the game's window was shown (see `hold_time`).
    splash_frames: u8,
}

impl StartupLoading {
    pub(crate) fn new() -> Self {
        Self { holds: Vec::new(), started: Instant::now(), done: false, first_frame: false, splash_frames: 0 }
    }

    /// Whether the hold `id` is still taken (tests, and holds that wait for another, `world_sources`).
    pub(crate) fn holding(&self, id: &str) -> bool {
        self.holds.iter().any(|(h, _)| h == id)
    }

    /// Keep the phase going until [`release`](Self::release) with the same id; `label` names the step ("Loading
    /// sound"). Ignored once the phase is over.
    pub(crate) fn hold(&mut self, id: &str, label: &str) {
        if !self.done && !self.holds.iter().any(|(h, _)| h == id) {
            self.holds.push((id.to_owned(), label.to_owned()));
        }
    }

    pub(crate) fn release(&mut self, id: &str) {
        let before = self.holds.len();
        self.holds.retain(|(h, _)| h != id);
        if self.holds.len() != before {
            mark(&format!("released {id}"));
        }
    }

    /// The phase is over (every hold released, the game window shown).
    pub(crate) fn done(&self) -> bool {
        self.done
    }

    fn step(&self) -> Option<&str> {
        self.holds.first().map(|(_, label)| label.as_str())
    }
}

/// The longest the loading phase keeps the game window hidden.
const MAX_HOLD: std::time::Duration = std::time::Duration::from_secs(180);

pub(crate) struct StartupLoadingPlugin;

impl Plugin for StartupLoadingPlugin {
    fn build(&self, app: &mut App) {
        let mut loading = StartupLoading::new();
        loading.hold("audio", "Loading sound");
        app.insert_resource(loading)
            .add_systems(Startup, |mut time: ResMut<Time<Virtual>>| time.pause())
            .add_systems(PreUpdate, hold_time.after(crate::graphics_menu::MenuInput))
            .add_systems(Update, track_audio);
    }
}

/// The audio hold goes when the runtime's first start is done (started or failed), or at once without an audio
/// install.
fn track_audio(content: Option<Res<crate::game_audio::AudioContent>>, library: Option<Res<crate::game_audio::Library>>, mut loading: ResMut<StartupLoading>) {
    if loading.done {
        return;
    }
    if library.is_none() || content.is_none_or(|c| !c.starting()) {
        loading.release("audio");
    }
}

/// Gameplay waits while a hold is up (after the menu's own pause rule, which runs every frame); when the last hold
/// goes the game window is shown and the splash closes, and time runs again unless the menu wants it paused.
fn hold_time(
    mut loading: ResMut<StartupLoading>,
    mut time: ResMut<Time<Virtual>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !loading.done && !loading.first_frame {
        loading.first_frame = true;
        mark("first frame");
    }
    if loading.done {
        if loading.splash_frames > 0 {
            loading.splash_frames -= 1;
            if loading.splash_frames == 0 {
                crate::splash::close();
            }
        }
        return;
    }
    // Never keep the window hidden for good: a hold that hangs gives way after a while (the work goes on behind).
    if !loading.holds.is_empty() && loading.started.elapsed() > MAX_HOLD {
        warn!("STARTUP_LOADING gave up waiting for {:?} after {} s; showing the game", loading.holds.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), MAX_HOLD.as_secs());
        loading.holds.clear();
    }
    if let Some(step) = loading.step() {
        crate::splash::stage(step);
        time.pause();
        return;
    }
    loading.done = true;
    if !background_window() {
        for mut w in &mut window {
            w.visible = true;
            w.focused = true;
        }
    }
    // The splash goes a few frames later, once the window is up and focused: closing it first left the foreground
    // to whatever window was behind it, and Windows then refused the game's focus request.
    loading.splash_frames = 3;
    if !menu.is_some_and(|m| m.open || m.fe_suspended) {
        time.unpause();
    }
    info!("STARTUP_LOADING done in {} ms", loading.started.elapsed().as_millis());
    mark("game shown");
    SHOWN.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_name_the_first_step_and_end_when_all_are_released() {
        let mut l = StartupLoading::new();
        l.hold("audio", "Loading sound");
        l.hold("mods", "Loading mods");
        l.hold("audio", "again");
        assert_eq!(l.holds.len(), 2);
        assert_eq!(l.step(), Some("Loading sound"));
        l.release("audio");
        assert_eq!(l.step(), Some("Loading mods"));
        l.release("mods");
        assert_eq!(l.step(), None);
    }
}

/// `SKATE_BACKGROUND_WINDOW=1` (muted test runs while someone plays on the desktop): the game window is created shown
/// but inactive and at the bottom of the window order, is never focused, and the splash is skipped.
pub(crate) fn background_window() -> bool {
    std::env::var("SKATE_BACKGROUND_WINDOW").is_ok_and(|v| v == "1")
}
