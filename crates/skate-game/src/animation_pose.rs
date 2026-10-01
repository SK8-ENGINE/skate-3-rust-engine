//! Production animation command evaluator. File IO and buffer ownership are
//! host services; frame selection, trajectory and pose math live in core.
use skate_core::animation::{
    output::{self, NativeMatrix, Sqt},
    playback_tree::PoseCommand,
    pose_add, pose_blend, pose_mirror,
    pose_sample::{sample_key, select_frames},
    pose_trajectory::{self, LoopTransform},
};
use skate_data::{
    animation_banks::AnimationBanks,
    animation_frames::{AnimationFrames, ClipFrames, SampleWords},
};
use std::path::Path;
mod authored_clips;

pub(crate) struct PoseEvaluator {
    pub frames: AnimationFrames,
    authored: authored_clips::Replacements,
    mod_clips: std::sync::RwLock<std::collections::BTreeMap<String, authored_clips::Replacements>>,
    body_bones: std::sync::OnceLock<Vec<usize>>,
    /// Endless Tricks: the body-hold clip a repeated rotation should park the legs in, and the
    /// compositions already built for it, keyed by the air clip they were built against.
    ///
    /// Composed on demand rather than from a table, because which air clip is playing -- the high
    /// or the low variant -- is chosen by an authored selector. Asking the clip that is actually
    /// playing is exact; a hardcoded list of air-clip names would be a guess, and guessing clip
    /// names is what put a hold pose on a repeat in the first place.
    endless_hold: std::sync::RwLock<Option<EndlessHold>>,
}

/// The first rung that carries a held body. `rung` is 1-based and the hold needs `loops > 0`.
const FIRST_HELD_RUNG: u32 = 2;

/// How long the held body takes to arrive, in seconds of clip time.
///
/// Matched to `endless_flip::LOOP_BLEND_SECONDS`, which was swept against the same jolt measurement
/// and found to be the shortest window that hides a junction without costing air time.
const HOLD_EASE_SECONDS: f32 = 0.08;

#[derive(Default)]
struct EndlessHold {
    hold_clip: String,
    /// The rung being flown, so the hold can advance across rungs rather than within one.
    rung: u32,
    /// The trick stem whose own clips the hold applies to, e.g. `360FLIP`.
    ///
    /// Without it the hold reached every clip that played while a rung was live -- the catch, the
    /// out clip, `BLEND_LAND`, the ride-away -- so the skater rode off still holding the pose. The
    /// hold belongs to the rotation, not to the rung.
    trick: String,
    composed: std::collections::BTreeMap<String, ClipFrames>,
}

impl PoseEvaluator {
    pub fn load(asset_root: &Path) -> Result<Self, String> {
        let mut evaluator = Self::from_banks(&AnimationBanks::load(asset_root)?)?;
        evaluator.load_authored_clips(asset_root)?;
        Ok(evaluator)
    }
    pub fn from_banks(banks: &AnimationBanks) -> Result<Self, String> {
        Ok(Self {
            frames: AnimationFrames::from_banks(banks)?,
            authored: Default::default(),
            mod_clips: Default::default(),
            body_bones: Default::default(),
            endless_hold: Default::default(),
        })
    }

    pub fn load_authored_clips(&mut self, root: &Path) -> Result<(), String> {
        self.authored = authored_clips::Replacements::load(root, &self.frames)?;
        Ok(())
    }

    /// Sample the composed body hold for `stock`, composing it on first use.
    ///
    /// A composition that cannot be built drops the hold rather than failing the frame: the result
    /// is the stock performance, which is the behaviour this replaces, and the game keeps running.
    /// It says so once, because a hold that silently does nothing is the bug being fixed here.
    fn sample_endless_hold(
        &self,
        stock: &ClipFrames,
        time: f32,
        previous_time: f32,
    ) -> Option<(Vec<Sqt>, Option<Sqt>)> {
        let mut held = self.endless_hold.write().ok()?;
        let state = held.as_mut()?;
        // Only the trick's own rotation is held. Its catch, its out clip and the landing are stock,
        // which is what lets the skater ride away in the ordinary animations.
        if !stock.name.contains(&state.trick) {
            return None;
        }
        if !state.composed.contains_key(&stock.name) {
            match authored_clips::hold_body(&self.frames, &stock.name, &state.hold_clip, state.rung) {
                Ok(composed) => {
                    state.composed.insert(stock.name.clone(), composed);
                }
                Err(error) => {
                    eprintln!(
                        "SKATE LIMITATION: no endless body hold for {} over {}: {error}",
                        stock.name, state.hold_clip
                    );
                    *held = None;
                    return None;
                }
            }
        }
        let clip = state.composed.get(&stock.name)?;
        let mut pose = sample_clip(clip, time).ok()?;
        // **Ease the hold in**, over the first held rung only.
        //
        // Measured, and worth stating precisely because the obvious reading is wrong. The worst
        // single-tick skeleton jump through a held 360 flip is 0.665 m against the authored trick's
        // own 0.227, and it is **not the deck**: the jolt probe reports the part index, and it is bone
        // 3. It is also identical with `SKATE_ENDLESS_RIGID_SPIN=0`, so the synthesised spin neither
        // causes it nor makes it worse.
        //
        // There are two mechanisms, not one. The larger is here -- the body arriving from mid-flick
        // into a held pose in a single tick, on the first held rung -- and easing it over `time`
        // measured 0.665 to 0.606 on the 360 flip and 0.664 to 0.474 on the laserflip. The remainder
        // is the air clip **restarting from frame 0** on every rung, which shows as a 0.27-0.43 m
        // step recurring on the clip's own 28-tick period and is covered, imperfectly, by the
        // authored cross-fade `endless_flip` installs. That one needs the rung's `InTime` plumbed
        // through so a rung can be baked knowing it will be cut early; it is recorded in
        // `docs/engine-defects.md` rather than half-fixed here.
        //
        // `time` is the ramp rather than a tick counter because a clip can be sampled more than once
        // inside a blend tree, which would double-count. Later rungs need no ease: the compositions
        // are continuous across rungs by construction.
        if state.rung <= FIRST_HELD_RUNG && time < HOLD_EASE_SECONDS {
            let weight = (time / HOLD_EASE_SECONDS).clamp(0.0, 1.0);
            if let Ok(from) = sample_clip(stock, time) {
                if from.len() == pose.len() {
                    let mut eased = pose.clone();
                    if pose_blend::blend(&from, &pose, weight, &mut eased).is_ok() {
                        pose = eased;
                    }
                }
            }
        }
        let previous = self
            .frames
            .has_trajectory
            .then(|| sample_bone(clip, previous_time, 0).ok())
            .flatten();
        Some((pose, previous))
    }

    /// Endless Tricks: park the body in `hold` while any clip plays, or clear the hold.
    ///
    /// Called every tick from the animation phase, so it must stay cheap when nothing changes: the
    /// compositions are kept across ticks and only dropped when the hold clip itself changes.
    pub(crate) fn set_endless_hold(&self, hold: Option<(&str, &str, u32)>) {
        let Ok(mut current) = self.endless_hold.write() else {
            return;
        };
        match (hold, current.as_ref()) {
            // The rung is part of the identity: each one carries the body loop a little further, so
            // a new rung needs its own composition rather than the previous rung's.
            (Some((name, _, rung)), Some(live)) if live.hold_clip == name && live.rung == rung => {}
            (Some((name, trick, rung)), _) => {
                *current = Some(EndlessHold {
                    hold_clip: name.to_owned(),
                    rung,
                    trick: trick.to_owned(),
                    composed: Default::default(),
                })
            }
            (None, _) => *current = None,
        }
    }

    /// Reuses the existing constrained body-clip parser; never replaces board/trajectory data.
    pub(crate) fn install_mod_clips(&self, owner: &str, text: &str) -> Result<(), String> {
        let clips = authored_clips::Replacements::parse(text, &self.frames)?;
        let mut all = self
            .mod_clips
            .write()
            .map_err(|_| "Animation override lock poisoned")?;
        for (id, other) in all.iter() {
            if id != owner && clips.0.keys().any(|k| other.0.contains_key(k)) {
                return Err(format!("Animation slots conflict with mod {id}"));
            }
        }
        all.insert(owner.to_owned(), clips);
        Ok(())
    }
    pub(crate) fn remove_mod_clips(&self, owner: &str) {
        if let Ok(mut all) = self.mod_clips.write() {
            all.remove(owner);
        }
    }
    pub(crate) fn clear_mod_clips(&self) {
        if let Ok(mut all) = self.mod_clips.write() {
            all.clear();
        }
    }

    /// The rig's body bones: everything from the hips down to the board root, plus the
    /// reparented hand and toe helpers. This is the same split `authored_clips` uses to replace a
    /// body animation while leaving the board's trajectory alone, which is what makes it possible
    /// to hold a pose while the board keeps moving underneath it.
    pub(crate) fn body_bones(&self) -> &[usize] {
        self.body_bones.get_or_init(|| {
            let index = |name: &str| self.frames.bone_names.iter().position(|n| n == name);
            let (Some(hips), Some(board)) = (index("HIPS"), index("SKATEBOARD_ROOT")) else {
                return Vec::new();
            };
            let helpers = [
                "RIGHTTOEBASE_REPARENTED",
                "LEFTTOEBASE_REPARENTED",
                "RIGHTHAND_REPARENTED",
                "LEFTHAND_REPARENTED",
            ];
            (hips..board).chain(helpers.iter().filter_map(|n| index(n))).collect()
        })
    }

    /// Executes the ordered stock tree. This uses the native immediate ACS
    /// arithmetic; the host does not recreate packed animation job commands.
    pub fn evaluate(&self, commands: &[PoseCommand]) -> Result<Vec<Sqt>, String> {
        let mod_clips = self
            .mod_clips
            .read()
            .map_err(|_| "Animation override lock poisoned")?;
        let mut stack: Vec<Vec<Sqt>> = Vec::new();
        for command in commands {
            match command {
                PoseCommand::Pose { name } => {
                    // Init82B97E38 retains these references from the actor's
                    // initial database. AddBindPose82B98118 reuses them even
                    // when a later motion clip comes from another bank.
                    stack.push(
                        self.frames
                            .named_pose(name)?
                            .samples
                            .iter()
                            .copied()
                            .map(sqt)
                            .collect(),
                    );
                }
                PoseCommand::Add { motion_is_a } => {
                    let other = stack
                        .pop()
                        .ok_or("Animation add has no reference subtree")?;
                    let motion = stack
                        .last_mut()
                        .ok_or("Animation add has no motion subtree")?;
                    if motion.len() != other.len() {
                        return Err("Animation add bone counts differ".into());
                    }
                    for (motion, other) in motion.iter_mut().zip(other) {
                        *motion = if *motion_is_a {
                            pose_add::add(*motion, other, true)
                        } else {
                            pose_add::add(other, *motion, false)
                        };
                    }
                }
                PoseCommand::Mirror { trajectory_mode } => {
                    let pose = stack.last_mut().ok_or("Animation mirror has no subtree")?;
                    pose_mirror::mirror(
                        pose,
                        &self.frames.parents,
                        &self.frames.mirror_indices,
                        *trajectory_mode,
                    )?;
                }
                PoseCommand::Clip {
                    name,
                    time,
                    previous_time,
                    loops,
                } => {
                    let stock = self.frames.clip(name)?;
                    trace_clip(&stock.name);
                    // Every substitution path copies the stock clip's loop transform, so the
                    // trajectory terms are read from the stock clip whichever one is sampled.
                    let (mut pose, previous) = match self.sample_endless_hold(
                        stock,
                        *time,
                        *previous_time,
                    ) {
                        Some(sampled) => sampled,
                        None => {
                            let clip = mod_clips
                                .values()
                                .find_map(|c| c.clip(&stock.name))
                                .or_else(|| self.authored.clip(&stock.name))
                                .unwrap_or(stock);
                            let pose = sample_clip(clip, *time)?;
                            let previous = self
                                .frames
                                .has_trajectory
                                .then(|| sample_bone(clip, *previous_time, 0))
                                .transpose()?;
                            (pose, previous)
                        }
                    };
                    if let Some(previous) = previous {
                        pose[0] = pose_trajectory::delta(
                            pose[0],
                            previous,
                            (*loops != 0).then_some(LoopTransform {
                                rotation: stock.loop_rotation_bits.map(f32::from_bits),
                                translation: stock.loop_translation_bits.map(f32::from_bits),
                            }),
                        );
                    }
                    stack.push(pose);
                }
                PoseCommand::WeightedBlend { weights } => {
                    let start = stack
                        .len()
                        .checked_sub(weights.len())
                        .ok_or("Weighted blend pose stack underflow")?;
                    let pose =
                        skate_core::animation::pose_blend::weighted(&stack[start..], weights)
                            .map_err(|e| format!("Weighted blend: {e:?}"))?;
                    stack.truncate(start);
                    stack.push(pose);
                }
                PoseCommand::Blend { weight } | PoseCommand::ChannelBlend { weight, .. } => {
                    let second = stack.pop().ok_or("Animation blend has no second subtree")?;
                    let first = stack
                        .last_mut()
                        .ok_or("Animation blend has no first subtree")?;
                    if first.len() != second.len() {
                        return Err("Animation subtree bone counts differ".into());
                    }
                    for (a, b) in first.iter_mut().zip(second) {
                        *a = match command {
                            PoseCommand::ChannelBlend {
                                use_channels_from_weights,
                                ..
                            } => pose_blend::channel_blend_sample(
                                *a,
                                b,
                                *weight,
                                *use_channels_from_weights,
                            ),
                            _ => pose_blend::blend_sample(*a, b, *weight),
                        };
                    }
                }
            }
        }
        if stack.len() != 1 {
            return Err(format!("Animation evaluation left{} poses", stack.len()));
        }
        Ok(stack.pop().unwrap())
    }

    /// TU3828D3800 then828D3B58. NewACS824744B8 initializes the
    /// detached parent to0, so trajectory deltas do not move the local rig.
    pub fn hierarchy(&self, pose: &[Sqt]) -> Result<Vec<NativeMatrix>, String> {
        if pose.len() != self.frames.parents.len() {
            return Err("Animation pose and hierarchy bone counts differ".into());
        }
        let mut globals: Vec<_> = pose.iter().copied().map(output::sqt_to_matrix).collect();
        output::compose_hierarchy_in_place(
            globals.len() as i32,
            &self.frames.parents,
            0,
            &mut globals,
        )
        .map_err(|e| format!("Invalid stock animation hierarchy: {e:?}"))?;
        Ok(globals)
    }
}

/// Names every distinct bank clip sampled, once each, under `SKATE_CLIP_TRACE`.
///
/// The graph names trees -- `B_360FLIP_A`, `B_KICKFLIP_CYC2` -- while the bank holds the clips a
/// selector resolves those to. Guessing across that gap is what put a hold pose on a repeat, so the
/// gap is instrumented rather than inferred.
fn trace_clip(name: &str) {
    static SEEN: std::sync::OnceLock<Option<std::sync::Mutex<std::collections::BTreeSet<String>>>> =
        std::sync::OnceLock::new();
    let seen = SEEN.get_or_init(|| std::env::var_os("SKATE_CLIP_TRACE").map(|_| Default::default()));
    if let Some(seen) = seen {
        if let Ok(mut seen) = seen.lock() {
            if seen.insert(name.to_owned()) {
                eprintln!("SKATE_CLIP {name}");
            }
        }
    }
}

fn sample_clip(clip: &ClipFrames, time: f32) -> Result<Vec<Sqt>, String> {
    let selection = select_frames(
        time,
        f32::from_bits(clip.fps_bits),
        clip.frames.len(),
        true,
        0.0,
    )?;
    Ok(clip.frames[selection.first]
        .iter()
        .zip(&clip.frames[selection.second])
        .enumerate()
        .map(|(bone, (&first, &second))| {
            // ClipFrames already contains independently decoded keys. The raw
            // VBR block-boundary compatibility path substitutes the next key,
            // which skips then holds a frame when applied to this flat cache.
            let mut sample = sample_key(sqt(first), sqt(second), selection);
            sample.translation[3] = f32::from_bits(clip.channel_weights[bone]);
            sample
        })
        .collect())
}

fn sample_bone(clip: &ClipFrames, time: f32, bone: usize) -> Result<Sqt, String> {
    let selection = select_frames(
        time,
        f32::from_bits(clip.fps_bits),
        clip.frames.len(),
        true,
        0.0,
    )?;
    let mut sample = sample_key(
        sqt(clip.frames[selection.first][bone]),
        sqt(clip.frames[selection.second][bone]),
        selection,
    );
    sample.translation[3] = f32::from_bits(clip.channel_weights[bone]);
    Ok(sample)
}

fn sqt(words: SampleWords) -> Sqt {
    let [sx, sy, sz, x, y, z, w, tx, ty, tz] = words.map(f32::from_bits);
    Sqt {
        scale: [sx, sy, sz, 1.0],
        rotation: [x, y, z, w],
        translation: [tx, ty, tz, 1.0],
    }
}

#[cfg(test)]
#[path = "tests/animation_pose.rs"]
mod tests;
