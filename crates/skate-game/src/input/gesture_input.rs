//! GameInputManager82696030/826962D8 -> listener8259B878/8259B9D0 ->
//! Fill8259B1F0..B7D8. Runs on the existing 60 Hz gameplay input boundary.
use skate_core::{
    graph::intents::IntentMap,
    input::gesture::{Recognizer, Settings},
};
use skate_data::collections::Collections;
use std::path::Path;

pub(crate) struct GestureInput {
    recognizers: Vec<(usize, Recognizer)>,
    maximum_misses: [u8; 2],
    held_pattern: Option<String>,
    /// Last traced hold state, so `SKATE_GESTURE_TRACE` prints transitions rather than every tick.
    traced_held: bool,
}

/// How far the stick must still be deflected for a mod-only hold to survive. Traced holds sit around
/// 0.9 to 1.0, and the authored final coordinates are all beyond 0.8, so half deflection is generous
/// without counting a stick on its way back to centre.
const HOLD_MIN_DEFLECTION: f32 = 0.5;

/// Cosine of the widest angle off the gesture's final coordinate that still holds -- 60 degrees.
///
/// Wide enough to cover the drift that was losing the 360 pop shuvit (about 25 degrees), and still far
/// short of separating opposed pairs: `360PopShuvit` ends right and `FS360PopShuvit` ends left, 180
/// degrees apart, so neither can be mistaken for the other.
const HOLD_MAX_ANGLE_COS: f32 = 0.5;

/// A family the mod made holdable, as opposed to the four retail holds, whose behaviour is untouched.
fn mod_only_family(name: &str) -> bool {
    !["Kickflip", "Heelflip", "N_Kickflip", "N_Heelflip"].contains(&name)
        && skate_core::scoring::extension::by_family(&name.to_ascii_lowercase(), 2).is_some()
}

/// `SKATE_GESTURE_TRACE=1` reports what the recogniser made of the stick.
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SKATE_GESTURE_TRACE").is_some())
}
impl GestureInput {
    pub fn load(root: &Path) -> Result<Self, String> {
        let data = Collections::load(root)?;
        let misses = |key| -> Result<u8, String> {
            let field = data.field("recognizer", key, "NumTicksPatternNotInRangeBeforeCulling")?;
            if field.type_name != "EA::Reflection::UInt8" {
                return Err("Recognizer culling field must be UInt8".into());
            }
            u8::from_str_radix(&field.data, 16).map_err(|e| e.to_string())
        };
        let mut recognizers = Vec::new();
        // Original constructor82695A68 insertion order; each file competes
        // internally, and all winners are delivered to the listener in order.
        for (stick, file) in [
            (1, "skater.pat"),
            (1, "skater90.pat"),
            (1, "skaterN90.pat"),
            (1, "skater_air.pat"),
            (1, "skater_fingerflip.pat"),
            (0, "skaterls.pat"),
            (0, "skaterstep.pat"),
        ] {
            recognizers.push((
                stick,
                Recognizer::new(skate_data::gesture_patterns::load(
                    &root.join("private/stock/data/joystick").join(file),
                )?)?,
            ));
        }
        Ok(Self {
            recognizers,
            maximum_misses: [misses("left_stick")?, misses("right_stick")?],
            held_pattern: None,
            traced_held: false,
        })
    }

    pub fn publish(
        &mut self,
        axes: [[f32; 2]; 2],
        difficulty: u32,
        flags: u32,
        physical_state: u32,
        endless_families: bool,
        ag: &mut IntentMap,
    ) {
        // Native manager negates mapped Y. Component deadzone0.1 is initialized
        // by82F75F60 from820641A8, separately from cInputMap's own deadzones.
        let samples = axes.map(|[x, y]| [x, -y].map(|v| if v.abs() < 0.1 { 0.0 } else { v }));
        let mut events = Vec::new();
        let mut held = false;
        let mut holdable = false;
        for stick in [1, 0] {
            // Held checks precede *all* matches on this stick.
            for (_, recognizer) in self.recognizers.iter_mut().filter(|r| r.0 == stick) {
                if let Some(index) = recognizer.held(samples[stick]) {
                    if self.held_pattern.as_deref().is_some_and(|name| {
                        name.eq_ignore_ascii_case(&recognizer.patterns()[index].name)
                    }) {
                        held = true;
                    }
                }
            }
            for (_, recognizer) in self.recognizers.iter_mut().filter(|r| r.0 == stick) {
                if let Some(result) = recognizer.sample(
                    samples[stick],
                    Settings {
                        maximum_misses: self.maximum_misses[stick],
                        difficulty,
                    },
                ) {
                    let name = recognizer.patterns()[result.pattern].name.clone();
                    // Retail scopes holding to these four. Endless Tricks extends it to every family
                    // that has a rung table -- gated, because `HoldPattern` is read by the authored
                    // ActionGraph (trick.xml) and widening it unasked would change stock behaviour
                    // for those gestures.
                    //
                    // **The rung table decides, not a list of names here.** This used to name four
                    // stems and so only four of the twelve installed ladders could ever run: the
                    // other eight -- the hardflips, inward heelflips, pop shuvits and their nollie
                    // forms -- had rungs, had install sites, had air clips to compose a hold from,
                    // and simply never received `HoldPattern`. Asking
                    // `scoring::extension::by_family` is the same authority `endless_flip::has_ladder`
                    // uses, so the two cannot drift apart, and it keeps the structural guarantee that
                    // a 180 trick can never be held: `fspopshuvit` is absent from the table on
                    // purpose, and the `90_` variants are not in it either.
                    //
                    // Gesture names lowercase onto the table's family keys exactly -- `FS360PopShuvit`
                    // onto `fs360popshuvit`, `N_360InwardHeelflip` onto `n_360inwardheelflip`.
                    if ["Kickflip", "Heelflip", "N_Kickflip", "N_Heelflip"].contains(&name.as_str())
                        || (endless_families
                            && skate_core::scoring::extension::by_family(
                                &name.to_ascii_lowercase(),
                                2,
                            )
                            .is_some())
                    {
                        self.held_pattern = Some(name.clone());
                        holdable = true;
                    }
                    if trace() {
                        eprintln!(
                            "SKATE_GESTURE recognised={name} strength={:.2} holdable={holdable}                              endless_families={endless_families}",
                            result.strength,
                        );
                    }
                    events.push((name, result.strength));
                }
            }
        }
        // **A more forgiving hold, for the families retail never held at all.**
        //
        // Retail keeps a hold only while the stick stays within the pattern's authored tolerance of
        // its final coordinate. For the four tricks retail actually holds that is the behaviour and it
        // stays exactly as authored. For the twelve the mod adds, it is too strict to play: traced
        // from a real session, a 360 pop shuvit armed its hold at `[0.99, 0.00]` and lost it at
        // `[0.92, -0.39]` -- the stick had slid down into `[0.91, -0.42]`, which is where the kickflip
        // and 360 flip holds live, so muscle memory from the tricks that already worked pulled the
        // stick straight out of this one.
        //
        // So a mod-only family keeps its hold while the stick is still **held out in roughly the right
        // direction**: deflected past `HOLD_MIN_DEFLECTION` and within `HOLD_MAX_ANGLE` of the final
        // coordinate. That is what "still holding on the edge of the stick" means as a test. It cannot
        // change stock behaviour, because stock never publishes `HoldPattern` for these gestures --
        // the whole path is behind `endless_families`.
        if !held && endless_families {
            if let Some(name) = self.held_pattern.as_deref() {
                if mod_only_family(name) {
                    for (stick, recognizer) in self.recognizers.iter() {
                        let Some(pattern) = recognizer
                            .patterns()
                            .iter()
                            .find(|p| p.name.eq_ignore_ascii_case(name))
                        else {
                            continue;
                        };
                        let Some(&[tx, ty]) = pattern.points.last() else {
                            continue;
                        };
                        let [sx, sy] = samples[*stick];
                        let deflection = (sx * sx + sy * sy).sqrt();
                        let target = (tx * tx + ty * ty).sqrt();
                        if deflection < HOLD_MIN_DEFLECTION || target <= f32::EPSILON {
                            continue;
                        }
                        // Cosine of the angle between where the stick is and where the gesture ends.
                        let cosine = (sx * tx + sy * ty) / (deflection * target);
                        if cosine >= HOLD_MAX_ANGLE_COS {
                            held = true;
                            break;
                        }
                    }
                }
            }
        }
        if held {
            ag.insert("HoldPattern", 1.0);
        }
        // Why a hold did or did not engage, printed only when it changes. Reported from play: only the
        // kickflip and the 360 flip could be held, where the harness ladders all twelve from the
        // authored PAT coordinates -- so what the game sees from the stick is the unknown, and
        // guessing at it is what this replaces. `held_pattern` is the gesture the recogniser last
        // armed and `held` is whether the stick is still inside that pattern's final coordinate.
        if trace() && held != self.traced_held {
            self.traced_held = held;
            eprintln!(
                "SKATE_GESTURE hold={held} pattern={:?} axes=[{:.2} {:.2}]",
                self.held_pattern, samples[1][0], samples[1][1],
            );
        }
        for (name, strength) in events {
            if permitted(&name, flags, physical_state, ag) {
                ag.insert("Trick", 1.0);
                ag.insert(&name, 1.0);
                ag.insert("GestureSpeed", strength);
            }
        }
    }
}

/// Every gesture the Endless Tricks ladder installs must be one the player can actually hold.
///
/// This is the gap the owner found in play: twelve ladders were installed and logged, but only four
/// gestures were ever marked holdable, so eight of them could never run a single extra rung. Pairing
/// the catalogue against the rung table catches that without any asset or any graph.
#[cfg(test)]
mod endless_hold_tests {
    /// The gesture names that carry a rung table, taken from the real catalogue rather than retyped.
    fn holdable() -> Vec<&'static str> {
        crate::input::gesture_catalog::all_names()
            .filter(|n| {
                skate_core::scoring::extension::by_family(&n.to_ascii_lowercase(), 2).is_some()
            })
            .collect()
    }

    #[test]
    fn every_laddered_family_is_a_holdable_gesture() {
        let names = holdable();
        // Twelve families, each with a nollie form already counted among them.
        assert_eq!(
            names.len(),
            12,
            "expected twelve holdable laddered gestures, got {names:?}"
        );
        for expected in [
            "360Flip",
            "360Hardflip",
            "360InwardHeelflip",
            "360PopShuvit",
            "FS360PopShuvit",
            "Laserflip",
            "N_360Flip",
            "N_360Hardflip",
            "N_360InwardHeelflip",
            "N_360PopShuvit",
            "N_FS360PopShuvit",
            "N_Laserflip",
        ] {
            assert!(names.contains(&expected), "{expected} is not holdable: {names:?}");
        }
    }

    /// A 180 trick must not become holdable by accident. `fspopshuvit` is kept out of the rung table
    /// on purpose, and the `90_` variants are not in it either, so the lookup excludes both.
    #[test]
    fn a_half_rotation_gesture_never_becomes_endless() {
        let names = holdable();
        for forbidden in ["FS_Shuvit", "BS_Shuvit", "FSPopShuvit", "90_360PopShuvit"] {
            assert!(
                !names.contains(&forbidden),
                "{forbidden} must not be holdable: {names:?}"
            );
        }
    }

    /// The forgiving hold applies to the twelve the mod added and to none of retail's four.
    ///
    /// Retail's hold tolerance is authored and must stay exact; the mod's families have no authored
    /// hold at all, which is what makes widening theirs safe.
    #[test]
    fn only_the_mod_added_families_get_the_forgiving_hold() {
        for retail in ["Kickflip", "Heelflip", "N_Kickflip", "N_Heelflip"] {
            assert!(
                !super::mod_only_family(retail),
                "{retail} is a retail hold and must keep its authored tolerance"
            );
        }
        for added in holdable() {
            assert!(
                super::mod_only_family(added),
                "{added} should get the forgiving hold"
            );
        }
        for unrelated in ["Ollie", "FSPopShuvit", "Hardflip", "90_360PopShuvit"] {
            assert!(
                !super::mod_only_family(unrelated),
                "{unrelated} is not a laddered family"
            );
        }
    }

    /// The widened angle must not let one gesture answer for another.
    ///
    /// The pair that matters is `360PopShuvit`, which ends to the right, against `FS360PopShuvit`,
    /// which ends to the left. They are 180 degrees apart, so a 60-degree window cannot confuse them.
    #[test]
    fn the_widened_hold_still_separates_opposed_gestures() {
        let cos_between = |a: [f32; 2], b: [f32; 2]| {
            let la = (a[0] * a[0] + a[1] * a[1]).sqrt();
            let lb = (b[0] * b[0] + b[1] * b[1]).sqrt();
            (a[0] * b[0] + a[1] * b[1]) / (la * lb)
        };
        // The authored final coordinates, as `probe_where_to_hold_each_family` prints them.
        let right = [0.95, 0.25];
        let left = [-0.97, 0.26];
        assert!(
            cos_between(right, left) < super::HOLD_MAX_ANGLE_COS,
            "the shuvit pair is inside the hold window"
        );
        // And the drift that was losing the hold is now inside it: traced at [0.92, -0.39].
        assert!(
            cos_between(right, [0.92, -0.39]) >= super::HOLD_MAX_ANGLE_COS,
            "the traced drift should now keep the hold"
        );
        // A stick on its way back to centre must not hold.
        assert!(
            (0.2f32 * 0.2 + 0.1 * 0.1).sqrt() < super::HOLD_MIN_DEFLECTION,
            "a near-centred stick must release"
        );
    }
}

fn permitted(name: &str, flags: u32, state: u32, ag: &IntentMap) -> bool {
    let bit = |n: u32| (flags & (1u32 << n)) != 0u32;
    let member = |names: &[&str]| names.iter().any(|n| n.eq_ignore_ascii_case(name));
    if bit(5) && member(&["FingerFlip", "FS_Varial", "BS_Varial"]) {
        return false;
    }
    if bit(4)
        && (matches!(state, 200 | 201)
            || member(&[
                "L_F_Kickflip",
                "L_B_Kickflip",
                "L_F_Heelflip",
                "L_B_Heelflip",
                "L_FS_Shuvit",
                "L_BS_Shuvit",
            ]))
    {
        return false;
    }
    if member(&["FrontFlip", "BackFlip"])
        && (!(ag.contains_key("RightAirGrab") || ag.contains_key("LeftAirGrab"))
            || ag.contains_key("LeftPush")
            || ag.contains_key("RightPush")
            || bit(3))
    {
        return false;
    }
    if bit(8) && member(&["SlideFs180", "SlideBs180"]) {
        return false;
    }
    if bit(12)
        && !member(&[
            "Ollie",
            "Kickflip",
            "Heelflip",
            "Hardflip",
            "InwardHeelflip",
            "VarialKickflip",
            "VarialHeelflip",
            "PopShuvit",
            "FSPopShuvit",
            "360PopShuvit",
            "FS360PopShuvit",
        ])
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires private stock joystick and collection data"]
    fn stock_360_gesture_reaches_native_square_mapping() {
        let root = std::path::PathBuf::from(
            std::env::var_os("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"),
        );
        let mut input = GestureInput::load(&root).unwrap();
        let mut ag = IntentMap::new();
        let points = input.recognizers[0]
            .1
            .patterns()
            .iter()
            .find(|p| p.name == "360Flip")
            .unwrap()
            .points
            .clone();
        input.publish([[0.0; 2]; 2], 0, 0, 100, false, &mut ag);
        for _ in 0..30 {
            ag.clear();
            input.publish(
                [[0.0; 2], [points[0][0], -points[0][1]]],
                0,
                0,
                100,
                false,
                &mut ag,
            );
            assert!(!ag.contains_key("Trick"));
        }
        for point in &points[1..] {
            ag.clear();
            input.publish([[0.0; 2], [point[0], -point[1]]], 0, 0, 100, false, &mut ag);
        }
        assert!(ag.contains_key("360Flip"), "{ag:?}");
        assert_eq!(
            super::super::gesture_mapping::select(
                super::super::gesture_catalog::Group::Square,
                &ag,
                false
            ),
            Some("360Flip")
        );
        assert_eq!(ag.get("GestureSpeed"), Some(&1.0));
        println!("Authored scoop through all seven PAT recognizers: {ag:?}");
    }
}
