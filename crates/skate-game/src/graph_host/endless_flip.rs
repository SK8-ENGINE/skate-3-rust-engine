//! Endless Tricks: the flip ladder past the authored quad.
//!
//! **A deliberate extension beyond retail Skate 3, not a port fix.** Retail authors
//! `TrickInto -> Cyc1 -> Cyc2 -> Cyc3 -> Out4` and stops there because no fifth cycle state
//! exists. `Cyc3`'s only exit is a bare `WillExpire` to `End.Out.Out4`, and `Out4` carries no
//! precondition at all -- unlike `Out1`/`Out2`/`Out3`, whose hold / `TimeToLand` /
//! `IsBodyFlipping` gates end the ladder early. So nothing is capped in code and nothing in code
//! can be uncapped: the extra rung has to be authored.
//!
//! This module appends one transition per flip cycle state, pointing the state at itself and
//! guarded by the `EndlessFlipLoop` condition. That condition is false unless a mod turns it on,
//! so with the mod off the compiled graph behaves exactly as shipped.
//!
//! Re-entering a cycle state and replaying its clip is authored retail behaviour rather than an
//! invention: `T_Kickflip_Unique.xml` (the Mike Carroll and Gonzo signature kickflips) runs
//! `TrickCyc1`/`TrickCyc2`/`TrickCyc3` over the *same* clip, differing only in the scoring name.
//!
//! Appending is safe because `Binding::from_graph` requires only `child > parent` and unique
//! ownership -- not contiguity -- so new elements go on the end of the arena and nothing
//! renumbers.
use skate_core::animation::{output::attributes::AttributeName, skeleton_input::name::encode};
use skate_core::scoring::extension;
use skate_data::state_graph::{GraphAttribute, GraphElement, StateGraph, binding::Binding};

/// Live loop state on the MotionHost.
///
/// `enabled` is the only switch: false on every retail path, and the synthesised condition
/// returns false on its first line when it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EndlessFlip {
    pub enabled: bool,
    /// Extra cycles taken past the authored quad, counted by the `EndlessFlipAdvance` hook on
    /// the synthesised transition. Zero on every retail path.
    pub loops: u32,
    /// Cap from `TrainerTuning::endless_flip_max`.
    pub max_loops: u32,
    /// Family stem of the ladder being extended, e.g. `Kickflip`, recorded by the hook so the
    /// name override knows which rung table to read.
    pub trick: Option<String>,
    /// The authored top rung of that family; the live rung counts up from here.
    pub base_rung: u32,
    /// From `TrainerTuning::endless_air_check`. False keeps flipping while held and airborne.
    pub air_check: bool,
    /// Grounded ticks, so the rung survives its own landing and clears at the next takeoff.
    grounded: u32,
}

impl EndlessFlip {
    /// The rung the skater is on: the family's authored top rung plus the loops taken.
    pub fn rung(&self) -> u32 {
        self.base_rung.max(1) + self.loops
    }
    /// One more cycle past the authored ladder, counted as the loop transition is taken.
    pub fn advance(&mut self, trick: String, base_rung: u32) {
        self.loops = self.loops.saturating_add(1);
        self.trick = Some(trick);
        self.base_rung = base_rung;
    }
    /// The ladder is cleared at **takeoff**, not at landing.
    ///
    /// Clearing on the first grounded tick looked obvious and was wrong: the authored
    /// `ScoringTrick` leaf is still publishing as the skater touches down, so the name collapsed
    /// back to the base rung exactly as it banked -- a five-rotation 360 flip scoring as a plain
    /// one. A fixed grace period only moved the problem, because how long that tail runs depends
    /// on the clip. Holding the rung until the skater next leaves the ground is exact: it always
    /// outlasts the landing it was earned on, and the next trick always starts clean.
    pub fn land(&mut self) {
        self.grounded = self.grounded.saturating_add(1);
    }
    /// Airborne again, so any previous ladder is over.
    pub fn airborne(&mut self) {
        if self.grounded > 0 {
            self.loops = 0;
            self.trick = None;
        }
        self.grounded = 0;
    }

    /// The name an extra rung publishes in place of the authored one.
    ///
    /// Re-entering the cycle state replays the authored `<Trick>3` and `<Trick>4` leaves, so both
    /// are rewritten to the rung actually being flown. Matching on those two names specifically is
    /// what keeps the override off every other trick the skater might publish before landing.
    pub fn published_name(&self, published: AttributeName) -> Option<AttributeName> {
        if !self.enabled || self.loops == 0 {
            return None;
        }
        let trick = self.trick.as_deref()?;
        // A cycle ladder replays its `<Trick>3` and `<Trick>4` leaves; a single-clip trick
        // republishes its one bare name. Matching only those keeps the override off every other
        // trick the skater might publish before landing.
        let authored: Vec<AttributeName> = if self.base_rung >= 4 {
            vec![
                encode(format!("{trick}3").as_bytes()),
                encode(format!("{trick}4").as_bytes()),
            ]
        } else {
            vec![encode(trick.as_bytes())]
        };
        if !authored.contains(&published) {
            return None;
        }
        let rung = extension::by_family(trick, self.rung())?;
        Some(encode(rung.identifier.as_bytes()))
    }
}

/// One authored flip cycle state carrying an installed loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    /// Index into `StateGraph::elements` of the cycle state itself.
    pub state: usize,
    /// Base trick name recovered from the authored `ScoringTrick`: `Kickflip` from `Kickflip4`,
    /// or `360FLIP` from the clip. Names the hold intent and the extra rungs.
    pub trick: String,
    /// The top rung retail authors here: 4 for a cycle ladder, 1 for a single-clip trick.
    pub base_rung: u32,
}

/// Nine authored sites. Six cycle ladders -- `T_Kickflip.xml` included four times (`Kickflip`,
/// `Heelflip`, `N_Kickflip`, `N_Heelflip`) and `T_Kickflip_Unique.xml` twice (Mike Carroll,
/// Gonzo) -- plus four single-clip tricks on `T_TrickWithDarkCatch.xml` (`360Flip`, `Laserflip`
/// plus its nollie form -- the nollie 360 flip has no cycle clip so it is left out). A different
/// count means these are not the assets this was written
/// against, and quietly installing onto a changed graph would be worse than refusing.
/// The retail cycle ladders, recovered data, and exactly this many: `T_Kickflip.xml` four times
/// (`Kickflip`, `Heelflip`, `N_Kickflip`, `N_Heelflip`) and `T_Kickflip_Unique.xml` twice (Mike
/// Carroll, Gonzo). A different count means these are not the assets this was written against.
///
/// The single-clip ladders are deliberately not counted here: which exist is the rung table's
/// decision rather than a number maintained by hand, and `install` reports what it found.
pub const EXPECTED_CYCLE_SITES: usize = 6;

fn text<'a>(element: &'a GraphElement, key: &str) -> Option<&'a str> {
    element
        .attributes
        .iter()
        .find(|a| a.name == key)
        .map(|a| a.text.as_str())
}

fn named(element: &GraphElement, tag: &str, name: &str) -> bool {
    element.tag == tag && text(element, "name") == Some(name)
}

fn attribute(name: &str, value: &str) -> GraphAttribute {
    GraphAttribute {
        name: name.to_owned(),
        text: value.to_owned(),
        float_bits: 0,
        boolean_byte: 0,
    }
}

fn flag(name: &str) -> GraphAttribute {
    GraphAttribute {
        name: name.to_owned(),
        text: "true".to_owned(),
        float_bits: 0,
        boolean_byte: 1,
    }
}

fn seconds(name: &str, value: f32) -> GraphAttribute {
    GraphAttribute {
        name: name.to_owned(),
        text: format!("{value}"),
        float_bits: value.to_bits(),
        boolean_byte: 0,
    }
}

/// Cross-fade for a cycle ladder's loop junction, swept to parity with the authored junctions.
///
/// **Only the cycle ladders use it.** The single-clip families restart with `transType="play"`
/// instead, and the reason is the self-transition. A blend with `transitionUnder` nests the
/// incoming clip inside the live transition, so re-entering the same state every rung builds a
/// tower that never resolves: the outer clip stays finished, `crossed_end` never clears, and the
/// loop condition re-fires *every tick*. That is what made the rung counter race while a single
/// animation played on screen. `play` clears the current tree first, so each rung restarts
/// cleanly -- and a cycle clip closes its own loop, so it needs no cross-fade to hide a seam.
///
/// The cycle ladders keep the blend because it was swept to parity with the authored junctions,
/// not because `play` was measured worse for them -- it was tried and made no difference to the
/// kickflip's rungs or its landing. Keeping the tuned value is simply the smaller claim.
/// Swept against the measurement above: 0.00 leaves a 0.33 m cut, 0.05 still leaves small
/// cycle-clip spikes, and 0.08 is the shortest window where they disappear entirely and the
/// worst in-flip jolt is an authored one again. Longer only costs air time.
pub const LOOP_BLEND_SECONDS: f32 = 0.08;


/// The body-hold clip a repeated rotation parks the legs in.
///
/// These exist in the animation bank and **no authored state references them** -- retail shipped
/// cycle content for these tricks and never wired it up. They are *hold poses*, not rotations:
/// measured, `T_360FLIP_H_CYC` moves the board a tenth as much as the kickflip's cycle does, which
/// is why playing one on a repeat parked the deck and only the first rotation was ever visible.
/// So a repeat keeps the trick's own air clip, which is what turns the board, and this supplies
/// the body alone through `animation_pose`'s body/board split.
///
/// Every single-clip family uses this same hold, including `N_360FLIP`. An earlier note here said
/// `N_360FLIP` was excluded for having no cycle clip of its own; it is not excluded, and the clip it
/// lacks is no longer what a repeat plays.
///
/// The high variant is taken unconditionally. Retail picks high or low for the *air* clip through
/// an authored selector on trick height; there is no equivalent selector for the holds, so this is
/// a choice rather than a recovery.
pub fn hold_clip(trick: &str) -> String {
    // Whether a held leg actually clears a rotating board is a judgement only play can make, so the
    // source is switchable without a rebuild. `SKATE_ENDLESS_HOLD_CLIP` takes either a bare clip
    // name or `<TRICK>=<clip>` pairs separated by semicolons.
    static OVERRIDE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let configured = OVERRIDE.get_or_init(|| {
        std::env::var("SKATE_ENDLESS_HOLD_CLIP")
            .ok()
            .filter(|v| !v.is_empty())
    });
    if let Some(configured) = configured {
        for entry in configured.split(';') {
            match entry.split_once('=') {
                Some((family, clip)) if family.eq_ignore_ascii_case(trick) => {
                    return clip.to_owned();
                }
                Some(_) => {}
                None => return entry.to_owned(),
            }
        }
    }
    HOLD_BODY.to_owned()
}

/// The authored clip the held body is taken from, for every single-clip family.
///
/// This is the kickflip ladder's own cycle -- the pose retail built for legs held clear of a board
/// rotating underneath them, which is exactly the problem here. Owner-confirmed by play: each
/// trick's own `T_<trick>_H_CYC` left the legs too close and they clipped through the deck. It is
/// also why every family holds the *same* way, which is what was asked for.
///
/// The name is the bank clip, not the tree `B_KICKFLIP_CYC2` the graph names -- read out of
/// `SKATE_CLIP_TRACE` rather than guessed, because guessing across that gap is what put a hold
/// pose on a repeat to begin with.
pub const HOLD_BODY: &str = "T_HI_KICK_CYC2";


/// Whether a stem from `T_TrickWithDarkCatch.xml` gets a ladder.
///
/// The rung table decides, not a list kept here. A family has a ladder exactly when
/// `scoring::extension` gives it rungs, so the two cannot drift apart, and a trick that must not be
/// endless -- a pop shuvit turns the board 180, so counting its rungs in 720s would be a lie --
/// cannot acquire one by being added to the wrong place. It also means no clip-stem strings are
/// written out by hand: the graph supplies the stems and the table judges them.
fn has_ladder(stem: &str) -> bool {
    skate_core::scoring::extension::by_family(stem, 2).is_some()
}

/// The clip a state plays, from its first `PlayAnimation` child.
fn state_clip(source: &StateGraph, state: usize) -> Option<String> {
    source.elements[state].children.iter().find_map(|&c| {
        let e = &source.elements[c];
        named(e, "behaviour", "PlayAnimation")
            .then(|| text(e, "anim"))
            .flatten()
            .map(str::to_owned)
    })
}

/// The authored rungs a cycle ladder should replay, in order.
///
/// Re-entering `Cyc3` alone replays one clip whose end pose is authored to run into `_OUT4`, not
/// back into its own first frame, and the owner saw that as the legs and hands twitching in a
/// weird loop from the fifth rung on. The authored pair reads the way the first four rungs do:
/// `Cyc2`'s end pose is shaped to run into `Cyc3`, because that is the order retail plays them in.
///
/// The sibling is found by name rather than by string surgery on the clip, so a family whose
/// clips are not named `_CYC2`/`_CYC3` still resolves -- the Unique includes are exactly that case.
fn cycle_pair(source: &StateGraph, state: usize, name: &str) -> Vec<String> {
    let own = state_clip(source, state);
    let previous = cycle_sibling(source, state, name).and_then(|s| state_clip(source, s));
    // Replay order: the earlier rung first, so each pair runs the way the authored ladder does.
    previous.into_iter().chain(own).collect()
}

/// The `Cyc2` state beside a `Cyc3` anchor, found by name rather than by string surgery on the clip.
fn cycle_sibling(source: &StateGraph, state: usize, name: &str) -> Option<usize> {
    let sibling = format!("{}2", name.strip_suffix('3')?);
    let parents = parent_map(source);
    let parent = parents[state]?;
    source.elements[parent]
        .children
        .iter()
        .copied()
        .find(|&c| {
            source.elements[c].tag == "state"
                && text(&source.elements[c], "name") == Some(sibling.as_str())
        })
}



/// Child -> parent, which `GraphElement` does not carry.
fn parent_map(source: &StateGraph) -> Vec<Option<usize>> {
    let mut parents = vec![None; source.elements.len()];
    for (index, element) in source.elements.iter().enumerate() {
        for &child in &element.children {
            // A transition is appended carrying child indices that are pushed after it, so a scan
            // run mid-install would otherwise walk off the end.
            if let Some(slot) = parents.get_mut(child) {
                *slot = Some(index);
            }
        }
    }
    parents
}

/// `B_360FLIP_A` -> `360FLIP`. `encode` case-folds, so this matches the authored
/// `ScoringTrick trick="360Flip"` without needing to walk up to it.
fn single_clip_trick(source: &StateGraph, state: usize) -> Option<String> {
    // Each include carries a second `LeftGround` inside `GrindOutAssist` that also plays `_A`.
    // Only the real air state sequences into the clip; the assist one blends into it. Without
    // this the site count comes out at fourteen instead of ten.
    let anim = source.elements[state].children.iter().find_map(|&c| {
        let e = &source.elements[c];
        (named(e, "behaviour", "PlayAnimation") && text(e, "transType") == Some("sequence"))
            .then(|| text(e, "anim"))
            .flatten()
    })?;
    let stem = anim.strip_prefix("B_")?.strip_suffix("_A")?;
    has_ladder(stem).then(|| stem.to_owned())
}

/// The quad `ScoringTrick` sits on an inactive `Quad` child in the generic include and directly
/// on the cycle state in the Unique one, so both depths are searched.
fn quad_trick(source: &StateGraph, state: usize) -> Option<String> {
    let mut candidates = source.elements[state].children.clone();
    for &child in &source.elements[state].children {
        if source.elements[child].tag == "state" {
            candidates.extend_from_slice(&source.elements[child].children);
        }
    }
    candidates.into_iter().find_map(|index| {
        let element = &source.elements[index];
        if !named(element, "behaviour", "ScoringTrick") {
            return None;
        }
        Some(text(element, "trick")?.strip_suffix('4')?.to_owned())
    })
}

/// The authored clip-expiry exit, which must be the state's last child so that inserting ahead
/// of it keeps every other authored transition -- underflip and dark catch above all -- at its
/// original priority. Returns its `InTime` attribute so the loop can mirror the same window.
fn authored_exit(source: &StateGraph, state: usize) -> Option<GraphAttribute> {
    let &last = source.elements[state].children.last()?;
    if source.elements[last].tag != "transition" {
        return None;
    }
    let &expression = source.elements[last].children.first()?;
    if source.elements[expression].tag != "expression" {
        return None;
    }
    let conditions = &source.elements[expression].children;
    let &[only] = conditions.as_slice() else {
        return None;
    };
    let condition = &source.elements[only];
    if !named(condition, "condition", "WillExpire") {
        return None;
    }
    condition.attributes.iter().find(|a| a.name == "InTime").cloned()
}

/// Appends the loop transition to every flip cycle state in `source`.
///
/// Call between `StateGraph::load` and `Binding::from_graph`, on the MotionGraph only.
pub fn install(source: &mut StateGraph) -> Result<Vec<Site>, String> {
    let anchors: Vec<usize> = (0..source.elements.len())
        .filter(|&i| {
            let element = &source.elements[i];
            element.tag == "state"
                && matches!(
                    text(element, "name"),
                    Some("Cyc3" | "TrickCyc3" | "LeftGround")
                )
                && element
                    .children
                    .iter()
                    .any(|&c| named(&source.elements[c], "behaviour", "PlayAnimation"))
        })
        .collect();

    let mut sites = Vec::new();
    for state in anchors {
        // A cycle ladder tops out at the authored quad; a single-clip trick has just the one.
        let (trick, base_rung) = match quad_trick(source, state) {
            Some(trick) => (trick, 4),
            None => match single_clip_trick(source, state) {
                Some(trick) => (trick, 1),
                None => continue,
            },
        };
        let name = text(&source.elements[state], "name")
            .ok_or("Flip cycle state has no name to target")?
            .to_owned();
        // The lead-in the loop fires on, read before anything is appended.
        //
        // **This is also what makes an extension rung shorter than an authored one**, and it cannot
        // simply be reduced. The loop is racing the bare `WillExpire` it sits ahead of, so it has to
        // fire on that exit's own lead-in or lose the state -- waiting for `crossed_end` was measured
        // to drop the kickflip ladder back to four rungs. On the kickflip that lead-in is 0.05 s, 3
        // ticks of a 26-tick cycle clip, so each extension rung runs 23 ticks and the cycle therefore
        // repeats 13% more often than an authored one: the deck appears to speed up at the quad.
        //
        // Taking the authored *inter-cycle* lead-in instead was tried and measured to be a no-op: the
        // `Cyc3` anchor's sibling `Cyc2` has no bare `WillExpire` as its last transition, so there is
        // nothing to read, and where there is (`TrickCyc3`) it is the same 0.02 s. Closing the gap
        // needs the authored `Out4` exit held off while the ladder is live, which is graph surgery on
        // the path that lands a stock quad. Recorded in `docs/engine-defects.md`; not attempted here.
        let in_time = authored_exit(source, state).ok_or_else(|| {
            format!("Flip cycle state {state} ({trick}) has no bare WillExpire exit to sit ahead of")
        })?;
        if std::env::var_os("SKATE_ENDLESS_TRACE").is_some() {
            let seconds = |a: &GraphAttribute| f32::from_bits(a.float_bits);
            let own = authored_exit(source, state).map(|a| seconds(&a));
            let sibling =
                cycle_sibling(source, state, &name).and_then(|s| authored_exit(source, s)).map(|a| seconds(&a));
            eprintln!(
                "SKATE_ENDLESS_SITE {trick} state={name} base={base_rung} chosen={:.4}s own_exit={own:?} sibling_exit={sibling:?}",
                seconds(&in_time),
            );
        }
        let source_offset = source.elements[state].source_offset;
        // A cycle ladder replays the authored pair; a single-clip family has no pair to replay and
        // replays its own air clip with a composed body instead. Read **before** anything is appended: the
        // transition element below is pushed carrying child indices that do not exist yet, so a
        // parent scan run after it walks off the end of `elements`.
        let pair = if base_rung >= 4 {
            cycle_pair(source, state, &name)
        } else {
            Vec::new()
        };

        // transition -> expression -> condition, appended in that order so each child index is
        // greater than its parent's, which is all `Binding::from_graph` requires.
        let transition = source.elements.len();
        let expression = transition + 1;
        let condition = transition + 2;
        let hook = transition + 3;
        let blend = transition + 4;
        source.elements.push(GraphElement {
            source_offset,
            tag: "transition".to_owned(),
            attributes: vec![attribute("target", &name)],
            children: vec![expression, hook, blend],
        });
        source.elements.push(GraphElement {
            source_offset,
            tag: "expression".to_owned(),
            attributes: Vec::new(),
            children: vec![condition],
        });
        source.elements.push(GraphElement {
            source_offset,
            tag: "condition".to_owned(),
            attributes: vec![
                attribute("name", "EndlessFlipLoop"),
                attribute("trick", &trick),
                attribute("baseRung", &base_rung.to_string()),
                in_time,
            ],
            children: Vec::new(),
        });
        source.elements.push(GraphElement {
            source_offset,
            tag: "hook".to_owned(),
            attributes: vec![
                attribute("name", "EndlessFlipAdvance"),
                attribute("trick", &trick),
                attribute("baseRung", &base_rung.to_string()),
                attribute("cycleClips", &pair.join(",")),
            ],
            children: Vec::new(),
        });
        // Retail's own mechanism for exactly this: override the next play's transition so the
        // re-entered cycle cross-fades out of the pose on screen rather than cutting to frame 0.
        source.elements.push(GraphElement {
            source_offset,
            tag: "hook".to_owned(),
            attributes: if base_rung >= 4 {
                // The cycle ladders keep the cross-fade they were swept to parity with.
                vec![
                    attribute("name", "OverideNextAnimTransitionHook"),
                    attribute("transType", "blend"),
                    seconds("time", LOOP_BLEND_SECONDS),
                    flag("transitionUnder"),
                    flag("blendWithCurrentFrame"),
                ]
            } else {
                // Blended, but **without** `transitionUnder`. That flag is what nested the
                // incoming clip inside the live transition and built a tower that never resolved,
                // leaving the outer clip finished, `crossed_end` uncleared and the loop re-firing
                // every tick. Restarting with `play` avoided that, and was invisible only while the
                // board was parked; now a rung turns the deck again, so the cut back to frame 0
                // shows as the board jumping at every junction. A plain cross-fade covers the seam
                // without nesting anything.
                vec![
                    attribute("name", "OverideNextAnimTransitionHook"),
                    attribute("transType", "blend"),
                    seconds("time", LOOP_BLEND_SECONDS),
                    flag("blendWithCurrentFrame"),
                ]
            },
            children: Vec::new(),
        });
        source.elements[state].children.push(transition);
        sites.push(Site {
            state,
            trick,
            base_rung,
        });
    }

    let cycles = sites.iter().filter(|s| s.base_rung >= 4).count();
    if cycles != EXPECTED_CYCLE_SITES {
        return Err(format!(
            "Expected {EXPECTED_CYCLE_SITES} authored flip cycle states, found {cycles}"
        ));
    }
    let mut families: Vec<&str> = sites
        .iter()
        .filter(|s| s.base_rung < 4)
        .map(|s| s.trick.as_str())
        .collect();
    families.sort_unstable();
    families.dedup();
    if families.is_empty() {
        return Err("No single-clip flip state carried a rung table".into());
    }
    eprintln!("SKATE_ENDLESS single-clip ladders: {}", families.join(" "));
    Ok(sites)
}

/// Moves each installed loop ahead of the authored exit it must beat.
///
/// `Binding::from_graph` walks elements in index order, so an appended transition lands last in
/// its owner's list, and `search_transitions` takes the first activatable one -- which would let
/// the authored exit win every time. The authored exit is `install`'s anchor and was the last
/// transition before ours, so swapping the final pair is exactly the move. Underflip and dark
/// catch are earlier in the list and keep their priority.
pub fn reorder(binding: &mut Binding, sites: &[Site]) -> Result<(), String> {
    for site in sites {
        let state = binding
            .states
            .iter()
            .position(|s| s.element == site.state)
            .ok_or_else(|| format!("Flip cycle state {} vanished during binding", site.state))?;
        // The single-clip air state has three bare-`WillExpire` exits (InAir/Land/OnBoard), not
        // one, so swapping the final pair would leave the loop behind the first of them. Sit ahead
        // of the earliest clip-expiry exit instead, which keeps underflip and dark catch -- which
        // are not expiry exits -- at their authored priority in both shapes.
        let first_expiry = binding.states[state]
            .transitions
            .iter()
            .position(|&t| {
                binding.transitions[t]
                    .expression
                    .and_then(|e| binding.expressions.get(e))
                    .is_some_and(|e| {
                        e.children.len() == 1
                            && e.children.iter().any(|c| match c {
                                skate_data::state_graph::binding::Node::Operation(op) => binding
                                    .operations
                                    .get(*op)
                                    .is_some_and(|o| o.name.eq_ignore_ascii_case("WillExpire")),
                                _ => false,
                            })
                    })
            })
            .ok_or_else(|| format!("Flip state {} lost its expiry exit", site.state))?;
        let transitions = &mut binding.states[state].transitions;
        let ours = transitions.pop().ok_or("loop transition vanished")?;
        transitions.insert(first_expiry, ours);
    }
    Ok(())
}
