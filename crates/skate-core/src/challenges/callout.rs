//! The tutorial callout ("Coach Frank Says", hud2/hudtutorial): the instruction a ShowTutorialCalloutHUD /
//! HideTutorialCalloutHUD event writes (82D29B60, reset 82D29B18) and the HUD component update that turns it into the
//! movie's SetState values (82667A70, 82667B58, 82667958). Research b945 §2, b866 §1.
//! Details: docs/hails-additions/42-challenges.md#tutorial-callout

/// The movie's states (hudtutorial SetState, b866 §1).
pub const STATE_GESTURES: i32 = 10;
pub const STATE_TEXT: i32 = 11;
pub const STATE_TEXT_FIRST: i32 = 12;
pub const STATE_HIDE_TEXT: i32 = 13;
pub const STATE_STANCE: i32 = 77;
/// The generic HUD show / hide of a component.
pub const STATE_SHOW: i32 = 246;
pub const STATE_HIDE: i32 = 236;

/// Retail conventions a mod's tip data uses the same way.
pub const HIDE_TEXT: &str = "#";
pub const KEY_NONE: [&str; 2] = ["none", "tutorial_none"];
pub const KEY_CLEAR: [&str; 2] = ["clear", "tutorial_clear"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalloutParams {
    /// A text-only tip hides after this long shown (82667B58 compares its timer with 10000; seconds if that is ms,
    /// inferred).
    pub text_timeout: f32,
}

impl Default for CalloutParams {
    fn default() -> Self {
        Self { text_timeout: 10.0 }
    }
}

/// The component's instruction (T: +0xC text, +0x8C key, +0x10C text only).
#[derive(Clone, Debug, PartialEq)]
pub struct Instruction {
    /// The gesture record key (scoring_trick, else scoring_tutorial), or one of the magic keys.
    pub key: String,
    /// A text id or literal; `#` hides a text-only tip.
    pub text: String,
    pub text_only: bool,
    /// Bumped on every change (T vt+0x10, mark changed); the component compares it with what it last showed.
    pub revision: u32,
    /// The component's visible byte: Show sets it, Hide clears it.
    pub visible: bool,
}

impl Default for Instruction {
    /// 82D29B18: key `ollie`, no text, gesture mode.
    fn default() -> Self {
        Self { key: "ollie".into(), text: String::new(), text_only: false, revision: 0, visible: false }
    }
}

impl Instruction {
    /// 82D29B60: no key = a text-only tip (always marked changed); with a key only a different key or text changes
    /// anything (a repeat of the same callout does nothing). The key stays as it was in text-only mode.
    fn write(&mut self, key: Option<&str>, text: &str) {
        match key {
            None => {
                self.text_only = true;
                self.text = text.into();
                self.revision = self.revision.wrapping_add(1);
            }
            Some(k) => {
                self.text_only = false;
                if self.key != k || self.text != text {
                    self.text = text.into();
                    self.key = k.into();
                    self.revision = self.revision.wrapping_add(1);
                }
            }
        }
    }

    /// ShowTutorialCalloutHUD (type 20): arg1 key (empty = text only), arg2 text; the component becomes visible.
    pub fn show(&mut self, key: Option<&str>, text: &str) {
        self.write(key.filter(|k| !k.is_empty()), text);
        self.visible = true;
    }

    /// HideTutorialCalloutHUD (type 21, 82578A80): key `none`, text `#`, then the component invisible.
    pub fn hide(&mut self) {
        self.write(Some(KEY_NONE[0]), HIDE_TEXT);
        self.visible = false;
    }
}

/// The HUD component (W: +16 active, +52 timer, +57 shown, +58 stance) as it drives the movie.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Panel {
    pub params: CalloutParams,
    active: bool,
    /// The visible byte as last seen: a change shows or hides the component (a `clear` key deactivates it until
    /// the next show).
    visible: bool,
    shown: bool,
    timer: f32,
    seen: u32,
    stance_regular: Option<bool>,
}

impl Panel {
    /// One HUD update: the states to send the movie, in order.
    pub fn update(&mut self, t: &Instruction, dt: f32, stance_regular: bool) -> Vec<i32> {
        let mut out = Vec::new();
        if t.visible != self.visible {
            self.visible = t.visible;
            self.active = t.visible;
            out.push(if t.visible { STATE_SHOW } else { STATE_HIDE });
        }
        if !self.active {
            self.seen = t.revision;
            return out;
        }
        let changed = t.revision != self.seen;
        self.seen = t.revision;
        if t.text_only {
            if changed {
                if t.text == HIDE_TEXT {
                    out.push(STATE_HIDE_TEXT);
                    self.shown = false;
                } else {
                    out.push(if self.shown { STATE_TEXT } else { STATE_TEXT_FIRST });
                    self.shown = true;
                    self.timer = 0.0;
                }
            }
            if self.shown {
                self.timer += dt;
                if self.timer >= self.params.text_timeout {
                    out.push(STATE_HIDE_TEXT);
                    self.shown = false;
                }
            }
            return out;
        }
        self.shown = false;
        self.timer = 0.0;
        if changed {
            if KEY_CLEAR.contains(&t.key.as_str()) {
                self.active = false;
                out.push(STATE_HIDE);
                return out;
            }
            out.push(if KEY_NONE.contains(&t.key.as_str()) { STATE_TEXT } else { STATE_GESTURES });
        }
        if self.stance_regular.is_some_and(|s| s != stance_regular) {
            out.push(STATE_STANCE);
        }
        self.stance_regular = Some(stance_regular);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gesture_tip_shows_once_and_a_repeat_does_nothing() {
        let (mut t, mut p) = (Instruction::default(), Panel::default());
        t.show(Some("kickflip"), "ID_CHALLENGE_TUT_SKSC_11_HINT_N_01");
        assert_eq!(p.update(&t, 0.016, true), [STATE_SHOW, STATE_GESTURES]);
        t.show(Some("kickflip"), "ID_CHALLENGE_TUT_SKSC_11_HINT_N_01");
        assert!(p.update(&t, 0.016, true).is_empty());
        assert_eq!(p.update(&t, 0.016, false), [STATE_STANCE]);
    }

    #[test]
    fn a_text_tip_shows_first_then_refreshes_and_times_out() {
        let (mut t, mut p) = (Instruction::default(), Panel::default());
        t.show(None, "ID_CHALLENGE_TUT_SKSC_08_HINT_07");
        assert_eq!(p.update(&t, 0.0, true), [STATE_SHOW, STATE_TEXT_FIRST]);
        t.show(Some(""), "ID_CHALLENGE_TUT_SKSC_08_HINT_08");
        assert_eq!(p.update(&t, 1.0, true), [STATE_TEXT]);
        assert!(p.update(&t, 8.5, true).is_empty());
        assert_eq!(p.update(&t, 0.5, true), [STATE_HIDE_TEXT]);
        t.show(None, HIDE_TEXT);
        assert_eq!(p.update(&t, 0.0, true), [STATE_HIDE_TEXT]);
    }

    #[test]
    fn hide_sends_the_none_key_and_the_generic_hide_and_clear_hides() {
        let (mut t, mut p) = (Instruction::default(), Panel::default());
        t.show(None, "ID_X");
        p.update(&t, 0.0, true);
        t.hide();
        assert_eq!(p.update(&t, 0.0, true), [STATE_HIDE]);
        assert_eq!((t.key.as_str(), t.text.as_str(), t.text_only), ("none", "#", false));
        t.show(Some("tutorial_clear"), "ID_Y");
        assert_eq!(p.update(&t, 0.0, true), [STATE_SHOW, STATE_HIDE]);
        assert!(p.update(&t, 0.0, true).is_empty(), "stays cleared");
        t.hide();
        p.update(&t, 0.0, true);
        t.show(Some("tutorial_none"), "ID_Z");
        assert_eq!(p.update(&t, 0.0, true), [STATE_SHOW, STATE_TEXT]);
    }
}
