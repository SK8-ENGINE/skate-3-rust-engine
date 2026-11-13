//! The replay editor's session: mode and key bindings as data (826127B8, 82610F18; b386, b387, b389).
//! Details: docs/hails-additions/37-instant-replay.md#replay-editor-session

/// Retail's editor modes (the values of `+0x30`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditorMode {
    /// Playing the clip (RB from the timeline); stops at the clip's end.
    Playback = 1,
    /// LB held: the sticks move the clip's start / end markers.
    Trim = 2,
    /// Instant replay (Back during play opens here, `82610458`).
    Instant = 5,
    /// The main timeline (the editor proper).
    Timeline = 6,
    FollowKey = 7,
    TripodKey = 8,
    GameKey = 9,
    FreeKey = 10,
    /// The effects menu: Up / Down pick the item, Left / Right change its value, B commits.
    Effects = 11,
}

/// Retail FE key indices (`fe_input`: 0 Start, 1 Back, 2 A, 3 B, 4 Y, 5 X, 6 RB, 7 RT, 9 LB, 10 LT, 11 L3, 12..15
/// Up / Down / Left / Right, 18 / 19 D-pad Left / Right pressed, 32..35 D-pad Up / Down / Left / Right released,
/// 46 A released, 53 LB released).
pub mod key {
    pub const START: u8 = 0;
    pub const BACK: u8 = 1;
    pub const A: u8 = 2;
    pub const B: u8 = 3;
    pub const Y: u8 = 4;
    pub const X: u8 = 5;
    pub const RB: u8 = 6;
    pub const LB: u8 = 9;
    pub const L3: u8 = 11;
    pub const UP: u8 = 12;
    pub const DOWN: u8 = 13;
    pub const LEFT: u8 = 14;
    pub const RIGHT: u8 = 15;
    pub const DPAD_LEFT_PRESSED: u8 = 18;
    pub const DPAD_RIGHT_PRESSED: u8 = 19;
    pub const DPAD_UP_RELEASED: u8 = 32;
    pub const DPAD_DOWN_RELEASED: u8 = 33;
    pub const DPAD_LEFT_RELEASED: u8 = 34;
    pub const DPAD_RIGHT_RELEASED: u8 = 35;
    pub const LB_RELEASED: u8 = 53;
}

/// What a key does; the host carries out the action, the session only changes mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditorAction {
    /// Instant replay -> the timeline (A).
    OpenTimeline,
    /// Open the Finished menu (B on the timeline; save / upload / exit, research b387).
    Finished,
    TakePhoto,
    Play,
    /// LB pressed / released: trim with the sticks.
    TrimStart,
    TrimEnd,
    TogglePanel,
    RemoveMarker,
    DropMarker,
    JumpToStart,
    JumpToEnd,
    /// Push the help screen (FE 13).
    Help,
    EffectsPrevious,
    EffectsNext,
    EffectsDecrease,
    EffectsIncrease,
    /// Leave the effects menu keeping the values (B).
    EffectsCommit,
}

/// One binding: in `mode`, `key` does `action` and goes to `next` (None = stays).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub mode: EditorMode,
    pub key: u8,
    pub action: EditorAction,
    pub next: Option<EditorMode>,
}

/// Retail's proven bindings (b386 table; mode 5 D-pad Left / Right pressed jump to the ends, mode 6 the released
/// D-pad does).
pub fn retail_bindings() -> Vec<Binding> {
    use EditorAction::*;
    use EditorMode::*;
    let b = |mode, key, action, next| Binding { mode, key, action, next };
    vec![
        b(Instant, key::A, OpenTimeline, Some(Timeline)),
        b(Instant, key::DPAD_LEFT_PRESSED, JumpToStart, None),
        b(Instant, key::DPAD_RIGHT_PRESSED, JumpToEnd, None),
        b(Timeline, key::B, Finished, None),
        b(Timeline, key::X, TakePhoto, None),
        b(Timeline, key::RB, Play, Some(Playback)),
        b(Timeline, key::LB, TrimStart, Some(Trim)),
        b(Timeline, key::L3, TogglePanel, None),
        b(Timeline, key::DPAD_UP_RELEASED, RemoveMarker, None),
        b(Timeline, key::DPAD_DOWN_RELEASED, DropMarker, None),
        b(Timeline, key::DPAD_LEFT_RELEASED, JumpToStart, None),
        b(Timeline, key::DPAD_RIGHT_RELEASED, JumpToEnd, None),
        b(Timeline, key::START, Help, None),
        b(Trim, key::LB_RELEASED, TrimEnd, Some(Timeline)),
        b(Effects, key::UP, EffectsPrevious, None),
        b(Effects, key::DOWN, EffectsNext, None),
        b(Effects, key::LEFT, EffectsDecrease, None),
        b(Effects, key::RIGHT, EffectsIncrease, None),
        b(Effects, key::B, EffectsCommit, Some(Timeline)),
    ]
}

/// The session: current mode and the binding table (data; a mod may replace or extend it).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorSession {
    pub mode: EditorMode,
    pub bindings: Vec<Binding>,
}

impl EditorSession {
    /// Back during play opens the instant replay (`82610458` sets mode 5 on that route).
    pub fn instant() -> Self {
        Self { mode: EditorMode::Instant, bindings: retail_bindings() }
    }

    /// One key: the bound action, with the mode changed when the binding says so. None = not bound in this mode.
    pub fn press(&mut self, key: u8) -> Option<EditorAction> {
        let binding = *self.bindings.iter().find(|b| b.mode == self.mode && b.key == key)?;
        if let Some(next) = binding.next {
            self.mode = next;
        }
        Some(binding.action)
    }

    /// Playback reached the clip's end (mode 1 stops there, `82610F18`).
    pub fn playback_ended(&mut self) {
        if self.mode == EditorMode::Playback {
            self.mode = EditorMode::Timeline;
        }
    }
}
