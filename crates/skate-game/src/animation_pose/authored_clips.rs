//! Optional authored body clips. Stock graph timing, board, trajectory and attributes remain authoritative.
use super::*;
use bevy::math::{Mat4, Quat, Vec3};
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Replacements(pub(super) BTreeMap<String, ClipFrames>);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    bone_names: Vec<String>,
    clips: BTreeMap<String, AuthoredClip>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredClip {
    fps: f32,
    frames: Vec<Vec<[f32; 16]>>,
}
impl Replacements {
    pub fn load(root: &Path, frames: &AnimationFrames) -> Result<Self, String> {
        let path = root.join("private/custom/crouch-treflip.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Self::parse(&text, frames).map_err(|e| format!("{}: {e}", path.display()))
    }
    pub(super) fn parse(text: &str, frames: &AnimationFrames) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if file.version != 1 || file.bone_names != frames.bone_names || file.clips.len() > 32 {
            return Err("Authored clip format or skeleton mismatch".into());
        }
        let bind = &frames.named_pose("RIG_TPOSE")?.samples;
        let index = |name: &str| {
            frames
                .bone_names
                .iter()
                .position(|n| n == name)
                .ok_or_else(|| format!("Missing {name}"))
        };
        let hips = index("HIPS")?;
        let board = index("SKATEBOARD_ROOT")?;
        let helpers = [
            (index("RIGHTTOEBASE_REPARENTED")?, index("RIGHTTOEBASE")?),
            (index("LEFTTOEBASE_REPARENTED")?, index("LEFTTOEBASE")?),
            (index("RIGHTHAND_REPARENTED")?, index("RIGHTHAND")?),
            (index("LEFTHAND_REPARENTED")?, index("LEFTHAND")?),
        ];
        let mut output = BTreeMap::new();
        for (name, authored) in file.clips {
            let allowed = matches!(
                name.as_str(),
                "R_ANTIC_OLLIE_N_0_INTO"
                    | "R_ANTIC_OLLIE_N_0_CYC"
                    | "R_ANTIC_360SHUVIT_N_0_CYC"
                    | "360FLIP_D_HIGH_G"
                    | "360FLIP_D_HIGH_A"
                    | "360FLIP_D_LOW_G"
                    | "360FLIP_D_LOW_A"
            );
            if !allowed {
                return Err(format!("Unsupported authored slot {name}"));
            }
            let stock = frames.clip(&name)?;
            if authored.fps != f32::from_bits(stock.fps_bits)
                || authored.frames.len() != stock.frames.len()
            {
                return Err(format!("{name}: timing differs from stock slot"));
            }
            let mut baked = stock.frames.clone();
            for (row, absolute) in baked.iter_mut().zip(authored.frames) {
                if absolute.len() != frames.bone_names.len() {
                    return Err(format!("{name}: missing bones"));
                }
                let mut globals = Vec::with_capacity(absolute.len());
                for values in absolute {
                    let m = Mat4::from_cols_array(&values);
                    let (scale, rotation, _) = m.to_scale_rotation_translation();
                    if !m.is_finite()
                        || m.determinant() <= 0.0
                        || scale.min_element() < 0.9
                        || scale.max_element() > 1.1
                        || !rotation.is_finite()
                        || (m.w_axis.w - 1.).abs() > 0.001
                        || m.x_axis.w.abs() + m.y_axis.w.abs() + m.z_axis.w.abs() > 0.001
                    {
                        return Err(format!("{name}: invalid joint matrix"));
                    }
                    globals.push(m);
                }
                let board_globals = row_globals(row, frames, bind);
                compose_row(
                    row,
                    &mut globals,
                    &board_globals,
                    frames,
                    bind,
                    hips,
                    board,
                    &helpers,
                );
            }
            output.insert(
                name.clone(),
                ClipFrames {
                    name,
                    source_offset: stock.source_offset,
                    fps_bits: stock.fps_bits,
                    loop_translation_bits: stock.loop_translation_bits,
                    loop_rotation_bits: stock.loop_rotation_bits,
                    channel_animation: stock.channel_animation,
                    channel_weights: stock.channel_weights.clone(),
                    frames: baked,
                },
            );
        }
        Ok(Self(output))
    }
    pub fn clip(&self, name: &str) -> Option<&ClipFrames> {
        self.0.get(name)
    }
}

/// Where a clip's rotation lives and which axis the hold spins about, under `SKATE_CLIP_TRACE`.
///
/// This is the measurement that settled how to do this at all. It showed every bone of the board
/// rotating by the same amount -- trucks and wheels within a degree of each other -- which is what
/// established the deck as a rigid body and made spinning one pose the right answer instead of
/// resampling a clip that holds no whole number of revolutions. Kept because the next person to
/// doubt that should be able to re-measure it in one run rather than infer it.
fn trace_spin(
    clip: &ClipFrames,
    frames: &AnimationFrames,
    bind: &[SampleWords],
    board: usize,
    axis: Vec3,
) {
    if std::env::var_os("SKATE_CLIP_TRACE").is_none() {
        return;
    }
    let bones = frames.bone_names.len();
    let mut totals = vec![0f32; bones];
    for pair in clip.frames.windows(2) {
        let before = row_globals(&pair[0], frames, bind);
        let after = row_globals(&pair[1], frames, bind);
        for bone in 0..bones.min(before.len()).min(after.len()) {
            totals[bone] += Quat::from_mat4(&before[bone])
                .normalize()
                .angle_between(Quat::from_mat4(&after[bone]).normalize());
        }
    }
    let mut ranked: Vec<(usize, f32)> = totals.iter().copied().enumerate().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let top: Vec<String> = ranked
        .iter()
        .take(5)
        .map(|&(bone, degrees)| {
            format!(
                "{}={:.0}",
                frames.bone_names.get(bone).map_or("?", String::as_str),
                degrees.to_degrees()
            )
        })
        .collect();
    eprintln!(
        "SKATE_SPIN {} board={} axis=[{:.2} {:.2} {:.2}] top=[{}]",
        clip.name,
        frames.bone_names.get(board).map_or("?", String::as_str),
        axis.x,
        axis.y,
        axis.z,
        top.join(" ")
    );
}

/// How much of the hold clip one rung traverses, as a fraction.
///
/// Small on purpose: the hold is meant to read as held, with only slight natural movement before the
/// stick comes back in. Traversing the whole cycle per rung is what made it jitter.
/// `SKATE_ENDLESS_HOLD_SWEEP` tunes it, and 0 freezes the pose outright.
fn hold_sweep() -> f32 {
    std::env::var("SKATE_ENDLESS_HOLD_SWEEP")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| (0.0..=1.0).contains(v))
        .unwrap_or(0.12)
}

/// Multiplier on the **measured** spin rate. 1.0 is the plateau speed the authored trick rotates at.
///
/// The default is **below** 1.0 on the owner's judgement from play: the trick's own plateau is
/// 20.85 deg/frame, and held indefinitely that reads as spinning rather than floating. 0.70 of it is
/// about 875 deg/s, still clearly a 360 flip turning over but with air under it. Measured plateau
/// stays the reference point so the number means something; this says "seven tenths of the trick".
///
/// This used to be a divisor on the baked frame count, which was broken in both directions: the
/// clip's playback length comes from the bank metadata rather than from `ClipFrames`, and
/// `sample_clip` clamps, so a shortened clip froze the deck for the rest of the rung and a
/// lengthened one dropped its tail and jumped at the junction. The frame count is now always the
/// stock one and this scales the angle swept instead.
fn spin_rate() -> f32 {
    std::env::var("SKATE_ENDLESS_SPIN_RATE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| (0.1..=4.0).contains(v))
        .unwrap_or(0.70)
}

/// Per-rung slowing of the held spin. 1.0 never slows.
///
/// Applied as a rate per rung, with the angle accumulated across rungs, so the spin stays continuous:
/// rung `n + 1` begins exactly where rung `n` left off, one frame-step on.
///
/// **0.90, not the 0.97 this started at.** 0.97 is 3% per rung, which is a real ramp arithmetically
/// and invisible in play -- the owner watched it and asked whether there was a ramp at all. 0.90
/// halves the spin by the seventh rung and reaches the floor by about the eleventh, which reads as the
/// deck winding down. `MIN_SPIN_FRACTION` stops it ever reading as parked.
fn spin_decay() -> f32 {
    std::env::var("SKATE_ENDLESS_SPIN_DECAY")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| (0.5..=1.0).contains(v))
        .unwrap_or(0.90)
}

/// A measured rigid spin: which way the deck turns, and how fast, in radians per animation frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Spin {
    pub axis: Vec3,
    /// Radians per frame. Deliberately per *frame*, not per second: `sample_clip` maps time to a
    /// frame index with `fps` alone while the clip clock's length also carries `base_speed`, so
    /// anything reasoned in seconds can desynchronise from the index actually being baked.
    pub rate: f32,
}

/// The signed rotation `b` applied after `a`, as an axis-angle vector, in world space.
///
/// Two details carry the whole direction fix. The product is `b * a^-1` (left-multiplied), which is
/// a *world*-frame delta -- `a^-1 * b` would express the axis in the deck's own coordinates and be
/// wrong to use as a fixed world axis. And `b` is negated when it lies in the opposite hemisphere to
/// `a`, because a quaternion and its negation are the same rotation: without that, a delta comes back
/// as its 360-degree complement and the axis points backwards.
fn world_delta(a: Quat, b: Quat) -> Vec3 {
    let a = a.normalize();
    let b = b.normalize();
    let b = if a.dot(b) < 0.0 { -b } else { b };
    let delta = (b * a.inverse()).normalize();
    let (axis, angle) = delta.to_axis_angle();
    axis * angle
}

/// Measure the deck's spin from the authored board channel.
///
/// **Why this is measured rather than chosen, and what was measured.** Running
/// `cargo run -p skate-data --example spin_measure -- <assets>` over every clip in the family prints
/// the numbers below; `360FLIP_D_HIGH_A` is representative.
///
/// * The axis used to come from `Quat::to_axis_angle` of the net first-to-last-frame rotation. A
///   494-degree path is not recoverable from its endpoints, and measurement shows that axis is
///   **inverted**: `dot(endpoint, accumulated) = -0.60` on `_HIGH_A`, -0.76 on `_LOW_A`, -0.89 on
///   `GONZ_HIGH_A`. That is the owner's "the board goes in the wrong direction", as a number.
/// * Summing the per-frame rotation *vectors* cannot invert, because a signed sum has no axis-sign
///   ambiguity to resolve. That sum is `360.6` degrees about `[-0.44, 0.72, -0.53]` -- one clean
///   revolution about an axis tilted **43.6 degrees** from vertical, which is exactly the geometry of
///   a kickflip combined with a 360 shuvit. So a constant-rate turn about it reads as a 360 flip
///   tumbling rather than as a board spinning about one obvious axis.
/// * The *instantaneous* axis does wander: `|sum r| / sum |r|` is 0.70-0.73 across the family, near
///   the `1/sqrt(2)` a matched flip-plus-shuvit predicts, and the authored path is 494 degrees where
///   the net is 360. So this is an **idealisation**, chosen deliberately: replaying the authored
///   directions instead carries 15-49 degrees per frame of direction wobble plus a 20-101 degree
///   kink where the sequence repeats, and "floaty" and "smooth" is what was asked for. A fitted
///   yaw-plus-flip model was also tried and reconstructs the clip no better than 65 degrees, so it
///   buys nothing for the complexity.
/// * The rate is the mean of the fastest third of frames -- the plateau. The whole-clip mean is
///   0.74-0.90 of it, because the clip ramps in off the flick and the catch slows the deck at the
///   end, and one turn per clip length (what the code did before) is slower still: 654 degrees per
///   second against the plateau's 1251. That is why every rung read as sluggish.
///
/// Returns `None` when the board barely turns, so the caller can fall back to the authored motion
/// rather than synthesise from noise.
pub(super) fn measure_spin(rotations: &[Quat]) -> Option<Spin> {
    if rotations.len() < 3 {
        return None;
    }
    let deltas: Vec<Vec3> = rotations
        .windows(2)
        .map(|pair| world_delta(pair[0], pair[1]))
        .collect();
    let axis = deltas
        .iter()
        .fold(Vec3::ZERO, |sum, d| sum + *d)
        .normalize_or_zero();
    if axis.length_squared() <= f32::EPSILON {
        return None;
    }
    let mut magnitudes: Vec<f32> = deltas.iter().map(|d| d.length()).collect();
    magnitudes.sort_by(f32::total_cmp);
    let third = (magnitudes.len() / 3).max(1);
    let rate = magnitudes[magnitudes.len() - third..].iter().sum::<f32>() / third as f32;
    (rate > 1e-4).then_some(Spin { axis, rate })
}

/// Where rung `hold_index` starts, and how fast it turns, given a measured spin.
///
/// `hold_index` is 0 for the first *synthesised* rung. Returns `(base_angle, rate_per_frame)`.
/// Because the base accumulates every preceding rung's swept angle, the step across a rung junction
/// is the same as the step inside a rung -- so the spin does not have to complete a whole number of
/// turns per rung. That is what dissolves the loop-closure problem the four resampling attempts each
/// broke on: there is no loop to close, only an angle that keeps going up.
pub(super) fn spin_angle(spin: Spin, hold_index: u32, frames_per_rung: usize, decay: f32) -> (f32, f32) {
    // Summed rather than closed-form, because of the floor below: a floored rate no longer follows a
    // geometric series, and a closed form that ignored that would put the base angle somewhere the
    // preceding rungs did not actually reach -- which is a step at the junction, the exact artefact
    // this design exists to avoid. Sixteen iterations at most, once per rung, so the cost is nothing.
    let mut base = 0.0;
    let mut rate = rate_for(spin, 0, decay);
    for index in 0..hold_index {
        base += rate_for(spin, index, decay) * frames_per_rung as f32;
        rate = rate_for(spin, index + 1, decay);
    }
    (base, rate)
}

/// The spin never decays below this fraction of its measured rate.
///
/// A deck that slows to a crawl reads as *parked*, which is the failure that started this whole
/// thread -- and with twelve rungs available a per-rung multiplier reaches a crawl easily. At the
/// default decay the floor is first hit around the eleventh rung.
const MIN_SPIN_FRACTION: f32 = 0.25;

fn rate_for(spin: Spin, hold_index: u32, decay: f32) -> f32 {
    let decayed = spin.rate * decay.powi(hold_index as i32);
    decayed.max(spin.rate * MIN_SPIN_FRACTION)
}

/// Bone locals -> globals for one bank row.
fn row_globals(row: &[SampleWords], frames: &AnimationFrames, bind: &[SampleWords]) -> Vec<Mat4> {
    let mut globals = vec![Mat4::IDENTITY; row.len()];
    for bone in 1..row.len() {
        let parent = frames.parents[bone];
        let local = matrix(pose_add::add(sqt(row[bone]), sqt(bind[bone]), true));
        globals[bone] = if parent > 0 {
            globals[parent as usize] * local
        } else {
            local
        };
    }
    globals
}

/// Take a stock row's board and a second pose's body, and write the result back into the row.
///
/// The helpers are the subtle part and the reason this is one function rather than two: bones
/// 32-35 are reparented into the board's *moving* space, so they have to be snapped to the joints
/// they shadow **after** the board range has been restored from the stock clip. Freezing them
/// instead is what once glued the back foot to the deck.
fn compose_row(
    row: &mut [SampleWords],
    globals: &mut [Mat4],
    board_globals: &[Mat4],
    frames: &AnimationFrames,
    bind: &[SampleWords],
    hips: usize,
    board: usize,
    helpers: &[(usize, usize); 4],
) {
    // The board hierarchy the helpers are expressed in. Normally the row's own, but a synthesised
    // deck passes its own here so the reparented helpers follow the board they can actually see.
    for bone in board..helpers.iter().map(|p| p.0).min().unwrap() {
        globals[bone] = board_globals[bone];
    }
    for &(helper, joint) in helpers {
        globals[helper] = globals[joint];
    }
    for bone in (hips..board).chain(helpers.iter().map(|p| p.0)) {
        let parent = frames.parents[bone];
        let local = if parent > 0 {
            globals[parent as usize].inverse() * globals[bone]
        } else {
            globals[bone]
        };
        row[bone] = words(remove_reference(local, sqt(bind[bone])));
    }
}

/// Endless Tricks: the trick's own air clip for the board, a second bank clip for the body.
///
/// This is what makes a repeated 360 flip or laserflip visible. Retail authors no rotating hold for
/// that family -- `T_TrickWithDarkCatch.xml` has one air clip and no cycle states -- and the only
/// cycle content in the bank for it, `T_<trick>_H_CYC`, is a hold *pose*: measured, it moves the
/// board a tenth as much as the kickflip's cycle does. Playing it on a repeat therefore parked the
/// board, which is why only the first rotation was ever visible.
///
/// So the repeat keeps playing the air clip, which is the thing that actually turns the deck, and
/// only the body is replaced -- the legs park in the hold pose instead of re-performing the flick.
/// The hold is sampled proportionally, because it is longer than the air clip and a hold wants a
/// pose rather than a performance.
pub(super) fn hold_body(
    frames: &AnimationFrames,
    air: &str,
    hold: &str,
    rung: u32,
) -> Result<ClipFrames, String> {
    let bind = &frames.named_pose("RIG_TPOSE")?.samples;
    let index = |name: &str| {
        frames
            .bone_names
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| format!("Missing {name}"))
    };
    let hips = index("HIPS")?;
    let board = index("SKATEBOARD_ROOT")?;
    let helpers = [
        (index("RIGHTTOEBASE_REPARENTED")?, index("RIGHTTOEBASE")?),
        (index("LEFTTOEBASE_REPARENTED")?, index("LEFTTOEBASE")?),
        (index("RIGHTHAND_REPARENTED")?, index("RIGHTHAND")?),
        (index("LEFTHAND_REPARENTED")?, index("LEFTHAND")?),
    ];
    let stock = frames.clip(air)?;
    let held = frames.clip(hold)?;
    if held.frames.is_empty() || stock.frames.is_empty() {
        return Err(format!("{air}/{hold}: an empty clip cannot be composed"));
    }
    // **The deck is spun, not resampled.**
    //
    // Four attempts at retiming the authored frames each traded one artefact for another: replaying
    // from frame 0 replayed the flick, starting at an "at speed" frame only moved where the ramp sat,
    // evening out the rate surged at the seam because the clip holds no whole number of revolutions,
    // and cutting it to a window that closes chopped visible motion. The authored board channel
    // simply is not loopable material.
    //
    // It does not have to be. Measured from the clip itself, every bone of the board rotates by the
    // same amount -- `TRUCK_BACK` 494.3 degrees, `LEFT_WHEELBACK` 494.3, the wheels and
    // `SKATEBOARD_ROOT` the same to a tenth of a degree -- so the deck moves as one rigid body
    // through the trick. One pose of it, turned at a steady rate, is therefore all a hold needs, and
    // it is smooth by construction: nothing is resampled, there is no seam to cover and no window to
    // trim. `measure_spin` records which numbers that rests on and how they were taken.
    let board_rotations: Vec<Quat> = stock
        .frames
        .iter()
        .map(|row| {
            row_globals(row, frames, bind)
                .get(board)
                .map(|m| m.to_scale_rotation_translation().1)
                .unwrap_or(Quat::IDENTITY)
        })
        .collect();
    // The rate multiplier is folded in **here**, into the measured spin, rather than applied to the
    // per-rung rate afterwards. Applying it only to the rate left the accumulated base angle in
    // unscaled units, so every rung junction stepped by the difference -- 139 degrees where the rung
    // stepped 14. That was invisible while the multiplier defaulted to 1.0 and appeared the moment it
    // did not, which is what `the_held_deck_turns_at_a_steady_rate_and_carries_across_rungs` is for.
    let spin = measure_spin(&board_rotations).map(|s| Spin {
        rate: s.rate * spin_rate(),
        ..s
    });
    let axis = spin.map_or(Vec3::Y, |s| s.axis);
    trace_spin(stock, frames, bind, board, axis);

    // The anchor is the **last** authored frame, not the first.
    //
    // Frame 0 is the deck still at the feet, pre-flick, so freezing it there parked a spinning deck
    // inside a body whose legs are held clear -- a cause of the legs and the board intersecting that
    // is separate from, and survives, the helper bug below. The last frame is where the authored
    // rotation actually ends, so taking it as the rigid reference and starting the first synthesised
    // rung at angle zero makes the hand-off out of the real trick continuous in **orientation and
    // position both**, by construction rather than by cross-fade.
    let anchor = row_globals(&stock.frames[stock.frames.len() - 1], frames, bind);
    let pivot = anchor
        .get(board)
        .map_or(Vec3::ZERO, |m| m.w_axis.truncate());
    // Always the stock frame count. See `spin_rate`: a clip's playback length comes from the bank
    // metadata rather than from `ClipFrames`, and `sample_clip` clamps, so a shorter count froze the
    // deck for the rest of the rung and a longer one dropped its tail and jumped at the junction.
    let count = stock.frames.len();

    // The body rides one long loop across rungs rather than replaying a cycle inside each one, which
    // is what made it judder. A single rotation therefore shows almost no movement.
    let sweep = hold_sweep();
    let last_hold = held.frames.len() - 1;
    // `rung` is 1-based and the hold only exists from the second rung on, so the first *synthesised*
    // rung is index 0. This was unobservable while a rung was exactly a whole turn, because the base
    // angle it feeds was then invisible. It is observable now.
    let hold_index = rung.saturating_sub(2);
    let base_hold = hold_index as f32 * sweep;
    let (base_angle, rate) =
        spin.map_or((0.0, 0.0), |s| spin_angle(s, hold_index, count, spin_decay()));

    // **On by default**, and `SKATE_ENDLESS_RIGID_SPIN=0` goes back to replaying the authored board
    // motion -- which is what shipped while two bugs here were outstanding. Both are now fixed, and
    // both were worth the measurement that found them:
    //
    // * *Direction*: the axis came from the net rotation between the first and last frame. A
    //   ~494-degree path is not recoverable from its endpoints, and the measurement shows that axis is
    //   genuinely inverted -- `dot(endpoint, accumulated)` is -0.60 on `360FLIP_D_HIGH_A`, -0.76 on
    //   `_LOW_A`, -0.89 on `GONZ_HIGH_A`. That was the owner's "the board goes in the wrong
    //   direction", as a number. It is accumulated from the per-frame deltas now.
    // * *Legs wrapping*: the board locals were rewritten in a second pass over `board..row.len()`,
    //   which includes the four reparented helpers -- bones 32-35, which live in the board's
    //   **moving** space -- so the hands and toes were welded to the synthesised deck at their
    //   frame-0 offsets. Same class of fault as the one that once glued the back foot to the deck.
    //   The writeback stops at the first helper now, so `compose_row`'s snap is what survives.
    let synthesise =
        spin.is_some() && std::env::var("SKATE_ENDLESS_RIGID_SPIN").map_or(true, |v| v != "0");
    let mut baked: Vec<Vec<SampleWords>> = Vec::with_capacity(count);
    for position in 0..count {
        let through = position as f32 / count as f32;
        let mut row = if synthesise {
            let mut row = stock.frames[stock.frames.len() - 1].clone();
            // Carry the authored trajectory through. Repeating one frame's bone 0 for the whole rung
            // would zero the clip's root motion for the duration of the hold, which is a change to
            // behaviour nobody asked for and which no other part of this is responsible for.
            row[0] = stock.frames[position][0];
            row
        } else {
            stock.frames[position].clone()
        };
        let board_globals: Vec<Mat4> = if synthesise {
            let turned = Quat::from_axis_angle(axis, base_angle + rate * position as f32);
            let correction = Mat4::from_translation(pivot)
                * Mat4::from_quat(turned)
                * Mat4::from_translation(-pivot);
            anchor.iter().map(|m| correction * *m).collect()
        } else {
            row_globals(&row, frames, bind)
        };
        // Clamped, not wrapped. `fract()` sent the body back to the hold clip's first frame as soon
        // as `base_hold` passed 1, which happens on the ninth rung -- inside the default cap of
        // twelve -- and showed as the legs snapping, in the one channel a hold exists to keep still.
        let at =
            (((base_hold + sweep * through).min(1.0) * last_hold as f32) as usize).min(last_hold);
        if held.frames[at].len() != row.len() {
            return Err(format!("{air}/{hold}: skeletons differ"));
        }
        let mut globals = row_globals(&held.frames[at], frames, bind);
        compose_row(
            &mut row,
            &mut globals,
            &board_globals,
            frames,
            bind,
            hips,
            board,
            &helpers,
        );
        if synthesise {
            // **Stops at the first helper.** `compose_row` has just snapped bones 32-35 onto the body
            // joints they shadow; running over them here is exactly what wrapped the legs around the
            // deck, because their parents are in the board subtree.
            for bone in board..helpers.iter().map(|p| p.0).min().unwrap() {
                let parent = frames.parents[bone];
                let local = if parent > 0 {
                    board_globals[parent as usize].inverse() * board_globals[bone]
                } else {
                    board_globals[bone]
                };
                row[bone] = words(remove_reference(local, sqt(bind[bone])));
            }
        }
        baked.push(row);
    }

    Ok(ClipFrames {
        name: stock.name.clone(),
        source_offset: stock.source_offset,
        fps_bits: stock.fps_bits,
        loop_translation_bits: stock.loop_translation_bits,
        loop_rotation_bits: stock.loop_rotation_bits,
        channel_animation: stock.channel_animation,
        channel_weights: stock.channel_weights.clone(),
        frames: baked,
    })
}
fn matrix(s: Sqt) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::from_slice(&s.scale),
        Quat::from_array(s.rotation),
        Vec3::from_slice(&s.translation),
    )
}

fn remove_reference(local: Mat4, reference: Sqt) -> Sqt {
    let (scale, rotation, translation) = local.to_scale_rotation_translation();
    let inverse = Quat::from_array(reference.rotation).inverse();
    let scale = scale / Vec3::from_slice(&reference.scale);
    // AddSQT rotates translation without applying the reference scale.
    let translation = inverse * (translation - Vec3::from_slice(&reference.translation));
    Sqt {
        scale: [scale.x, scale.y, scale.z, 1.0],
        rotation: (inverse * rotation).normalize().to_array(),
        translation: [translation.x, translation.y, translation.z, 1.0],
    }
}

fn words(s: Sqt) -> SampleWords {
    [
        s.scale[0],
        s.scale[1],
        s.scale[2],
        s.rotation[0],
        s.rotation[1],
        s.rotation[2],
        s.rotation[3],
        s.translation[0],
        s.translation[1],
        s.translation[2],
    ]
    .map(f32::to_bits)
}

/// The measured spin, independent of any asset.
///
/// These need no banks and so are not `#[ignore]`d, unlike everything else in this file. That is
/// deliberate: both of the bugs these pin were found by eye, after a rebuild, by the owner. Each is
/// now a failing assertion in a suite that runs in a second.
#[cfg(test)]
mod spin_tests {
    use super::*;

    /// A clip whose board turns `total` radians about `axis` in `frames` even steps.
    fn turning(axis: Vec3, total: f32, frames: usize) -> Vec<Quat> {
        (0..frames)
            .map(|i| {
                Quat::from_axis_angle(axis.normalize(), total * i as f32 / (frames - 1) as f32)
            })
            .collect()
    }

    /// Bug A, pinned from both sides: the accumulated axis finds the direction and the endpoint
    /// expression the code used to use gets it backwards.
    ///
    /// The axes are chosen with a dominant negative component, because that is what decides the sign
    /// `Quat::to_axis_angle` hands back once the path passes a half turn -- which is why the old code
    /// was wrong for the 360 flip and right for a smaller rotation.
    #[test]
    fn the_measured_axis_follows_the_path_where_the_endpoints_invert_it() {
        // 494 degrees is the authored 360 flip's measured path.
        let total = 494f32.to_radians();
        for axis in [
            Vec3::new(-0.439, 0.724, -0.531),
            Vec3::new(-0.476, 0.746, -0.466),
            Vec3::new(-0.690, -0.642, 0.334),
            Vec3::new(0.9, -0.1, 0.2),
            Vec3::NEG_Y,
        ] {
            let axis = axis.normalize();
            let rotations = turning(axis, total, 33);
            let spin = measure_spin(&rotations).expect("a turning board has a spin");
            assert!(
                spin.axis.dot(axis) > 0.999,
                "measured {:?} against {axis:?}",
                spin.axis
            );
            let expected = total / 32.0;
            assert!(
                (spin.rate - expected).abs() < 1e-4,
                "rate {} against {expected}",
                spin.rate
            );

            // The expression this replaced, kept so the bug cannot come back quietly.
            let net = (*rotations.last().unwrap() * rotations[0].inverse()).normalize();
            let (endpoint, _) = net.to_axis_angle();
            assert!(
                endpoint.dot(axis) < 0.0,
                "{axis:?}: the endpoint axis was supposed to be inverted here, got {endpoint:?}"
            );
        }
    }

    #[test]
    fn reversing_the_path_reverses_the_measured_spin() {
        let axis = Vec3::new(-0.439, 0.724, -0.531).normalize();
        let forward = measure_spin(&turning(axis, 494f32.to_radians(), 33)).unwrap();
        let mut reversed = turning(axis, 494f32.to_radians(), 33);
        reversed.reverse();
        let back = measure_spin(&reversed).unwrap();
        assert!(forward.axis.dot(back.axis) < -0.999, "the spin did not reverse");
        assert!((forward.rate - back.rate).abs() < 1e-4);
    }

    /// The rate is the plateau, not the whole-clip mean. Measured: the authored clips ramp in off the
    /// flick and the catch slows the deck, so the mean is 0.74-0.90 of the plateau and one turn per
    /// clip length -- what the code used to do -- is slower still.
    #[test]
    fn the_rate_is_the_plateau_and_not_the_mean() {
        let axis = Vec3::Y;
        let plateau = 20.85f32.to_radians();
        // Ramp in over 6 frames, hold for 16, then halve for the catch, as the real profile does.
        let steps: Vec<f32> = (0..6)
            .map(|i| plateau * (i + 1) as f32 / 6.0)
            .chain(std::iter::repeat_n(plateau, 16))
            .chain(std::iter::repeat_n(plateau * 0.5, 10))
            .collect();
        let mut angle = 0.0;
        let mut rotations = vec![Quat::IDENTITY];
        for step in &steps {
            angle += step;
            rotations.push(Quat::from_axis_angle(axis, angle));
        }
        let spin = measure_spin(&rotations).unwrap();
        assert!(
            (spin.rate - plateau).abs() < 0.05 * plateau,
            "{} against a plateau of {plateau}",
            spin.rate
        );
        let mean = steps.iter().sum::<f32>() / steps.len() as f32;
        assert!(
            mean < 0.9 * plateau,
            "the fixture does not discriminate: mean {mean} plateau {plateau}"
        );
    }

    /// A still board has no spin to synthesise, so the caller keeps the authored motion.
    #[test]
    fn a_board_that_does_not_turn_has_no_spin() {
        assert!(measure_spin(&[Quat::IDENTITY; 20]).is_none());
        assert!(measure_spin(&[Quat::IDENTITY, Quat::IDENTITY]).is_none());
    }

    /// The continuity claim the whole design rests on: a rung joins the next with the same step it
    /// runs at internally, so the spin need not complete a whole number of turns per rung.
    #[test]
    fn rungs_join_with_the_step_they_turn_at() {
        let spin = Spin { axis: Vec3::Y, rate: 0.364 };
        let count = 33;
        for decay in [1.0, 0.90, 0.5] {
            for index in 0..12u32 {
                let (base, rate) = spin_angle(spin, index, count, decay);
                let (next_base, _) = spin_angle(spin, index + 1, count, decay);
                let last = base + rate * (count - 1) as f32;
                assert!(
                    (next_base - (base + rate * count as f32)).abs() < 1e-4,
                    "decay {decay} rung {index}: {next_base} is not {} ",
                    base + rate * count as f32
                );
                assert!(
                    (next_base - last - rate).abs() < 1e-3,
                    "decay {decay} rung {index}: the junction steps {} where the rung steps {rate}",
                    next_base - last
                );
            }
        }
    }

    /// The first synthesised rung starts where the authored trick ended -- angle zero -- so the
    /// hand-off needs no cross-fade to hide an orientation jump.
    #[test]
    fn the_first_held_rung_starts_at_the_authored_orientation() {
        let spin = Spin { axis: Vec3::Y, rate: 0.364 };
        let (base, rate) = spin_angle(spin, 0, 33, 0.97);
        assert_eq!(base, 0.0);
        assert!((rate - spin.rate).abs() < 1e-6, "the first rung must not be decayed");
    }

    #[test]
    fn the_spin_decays_monotonically_and_never_stops_or_reverses() {
        let spin = Spin { axis: Vec3::Y, rate: 0.364 };
        let mut previous = f32::INFINITY;
        for index in 0..12u32 {
            let (_, rate) = spin_angle(spin, index, 33, 0.90);
            assert!(rate > 0.0, "rung {index} stopped");
            assert!(rate <= previous, "rung {index} did not slow");
            previous = rate;
        }
        // Visible by the seventh rung, which is what 0.97 failed to be.
        let (_, seventh) = spin_angle(spin, 6, 33, 0.90);
        assert!(
            (0.4..0.65).contains(&(seventh / spin.rate)),
            "the seventh rung runs at {} of the first",
            seventh / spin.rate
        );
        // decay = 1 must be exactly constant.
        let (_, a) = spin_angle(spin, 0, 33, 1.0);
        let (_, b) = spin_angle(spin, 9, 33, 1.0);
        assert_eq!(a, b);
    }

    /// Scaling the rate must scale the base angle with it.
    ///
    /// The regression this pins reached play: the rate multiplier was applied to the per-rung rate but
    /// not to the accumulated base, so every junction stepped by the mismatch -- 139 degrees against a
    /// 14-degree rung. It hid for as long as the multiplier defaulted to 1.0. Linearity in the rate is
    /// the property that makes the multiplier safe wherever it is applied.
    #[test]
    fn the_angle_scales_with_the_rate() {
        for scale in [0.5, 0.7, 1.0, 2.0] {
            for index in 0..6u32 {
                let base_spin = Spin { axis: Vec3::Y, rate: 0.364 };
                let scaled = Spin { rate: base_spin.rate * scale, ..base_spin };
                let (base_a, rate_a) = spin_angle(base_spin, index, 33, 0.90);
                let (base_b, rate_b) = spin_angle(scaled, index, 33, 0.90);
                assert!(
                    (base_b - base_a * scale).abs() < 1e-3,
                    "scale {scale} rung {index}: base {base_b} is not {} ",
                    base_a * scale
                );
                assert!(
                    (rate_b - rate_a * scale).abs() < 1e-5,
                    "scale {scale} rung {index}: rate {rate_b} is not {}",
                    rate_a * scale
                );
            }
        }
    }

    /// The deck must never read as parked, however many rungs are flown. A deck that stops is the
    /// failure this whole mechanism was built to fix, so the floor is pinned rather than trusted.
    #[test]
    fn the_spin_never_decays_below_the_floor() {
        let spin = Spin { axis: Vec3::Y, rate: 0.364 };
        for decay in [0.5, 0.75, 0.90, 1.0] {
            for index in 0..16u32 {
                let (_, rate) = spin_angle(spin, index, 33, decay);
                assert!(
                    rate >= spin.rate * MIN_SPIN_FRACTION - 1e-6,
                    "decay {decay} rung {index}: {rate} is under the floor"
                );
            }
        }
        // At the hardest decay the floor binds early, and the junction must still be continuous --
        // which is why the base angle is summed rather than taken from a geometric closed form.
        let count = 33;
        for index in 0..15u32 {
            let (base, rate) = spin_angle(spin, index, count, 0.5);
            let (next, _) = spin_angle(spin, index + 1, count, 0.5);
            assert!(
                (next - (base + rate * count as f32)).abs() < 1e-3,
                "rung {index}: the floored junction is discontinuous"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (AnimationFrames, String) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let banks = AnimationBanks::load(&root).unwrap();
        let frames = AnimationFrames::from_banks(&banks).unwrap();
        let text =
            std::fs::read_to_string(root.join("private/custom/crouch-treflip.json")).unwrap();
        (frames, text)
    }
    #[test]
    #[ignore = "requires private stock banks and the captured crouch/treflip asset"]
    fn authored_body_preserves_board_trajectory_and_slot_timing() {
        let (frames, text) = fixture();
        let replacements = Replacements::parse(&text, &frames).unwrap();
        assert_eq!(replacements.0.len(), 7);
        let board = frames
            .bone_names
            .iter()
            .position(|n| n == "SKATEBOARD_ROOT")
            .unwrap();
        for (name, clip) in &replacements.0 {
            let stock = frames.clip(name).unwrap();
            assert_eq!(clip.fps_bits, stock.fps_bits);
            assert_eq!(clip.frames.len(), stock.frames.len());
            assert_eq!(clip.channel_weights, stock.channel_weights);
            for (a, b) in clip.frames.iter().zip(&stock.frames) {
                assert_eq!(a[0], b[0]);
                assert_eq!(&a[board..32], &b[board..32]);
            }
            assert!(
                clip.frames
                    .iter()
                    .zip(&stock.frames)
                    .any(|(a, b)| a[2] != b[2])
            );
        }
        assert!(replacements.clip("NOLLIE_360FLIP_D_HIGH_G").is_none());
    }
    #[test]
    #[ignore = "requires private stock banks and the captured crouch/treflip asset"]
    fn authored_idle_body_loop_closes() {
        let (frames, text) = fixture();
        let replacements = Replacements::parse(&text, &frames).unwrap();
        for name in ["R_ANTIC_OLLIE_N_0_CYC", "R_ANTIC_360SHUVIT_N_0_CYC"] {
            let c = replacements.clip(name).unwrap();
            assert_eq!(&c.frames[0][1..25], &c.frames.last().unwrap()[1..25]);
        }
    }
    /// The banks alone, from `SKATE3_ASSET_ROOT` -- the convention every other asset test here uses.
    ///
    /// The `fixture` above hardcodes `../../assets`, which a git worktree does not have, so those
    /// tests cannot run from one at all. The hold needs no authored json, only the stock banks.
    fn banks() -> AnimationFrames {
        let root = std::path::PathBuf::from(
            std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT"),
        );
        let banks = AnimationBanks::load(&root).unwrap();
        AnimationFrames::from_banks(&banks).unwrap()
    }

    /// Bug B, as an assertion rather than an eyeball: the held hands and toes track the **body**.
    ///
    /// Bones 32-35 are reparented into the board's *moving* space, so they are the one part of the
    /// rig that a synthesised deck can capture. It did: the board locals were written back over a
    /// range that included them, which welded them to the spinning deck at their frame-0 offsets, and
    /// the owner reported it as the legs wrapping around the board. Both halves are checked here --
    /// that each helper sits on the joint it shadows, and that it is *not* sitting where the spun
    /// deck would have put it -- because only the second half fails if the fix is reverted.
    #[test]
    #[ignore = "requires private stock animation banks"]
    fn the_held_hands_and_toes_follow_the_body_not_the_spun_deck() {
        let frames = banks();
        let bind = &frames.named_pose("RIG_TPOSE").unwrap().samples;
        let index = |name: &str| frames.bone_names.iter().position(|n| n == name).unwrap();
        let helpers = [
            (index("RIGHTTOEBASE_REPARENTED"), index("RIGHTTOEBASE")),
            (index("LEFTTOEBASE_REPARENTED"), index("LEFTTOEBASE")),
            (index("RIGHTHAND_REPARENTED"), index("RIGHTHAND")),
            (index("LEFTHAND_REPARENTED"), index("LEFTHAND")),
        ];
        let hold = crate::graph_host::endless_flip::HOLD_BODY;
        let truck = frames.bone_names.iter().position(|n| n == "TRUCK_FRONT").unwrap();
        for rung in 2..=5u32 {
            let composed = hold_body(&frames, "360FLIP_D_HIGH_A", hold, rung)
                .unwrap_or_else(|e| panic!("rung {rung}: {e}"));
            let rows: Vec<Vec<Mat4>> = composed
                .frames
                .iter()
                .map(|row| row_globals(row, &frames, bind))
                .collect();
            // Each helper sits on the joint it shadows. A centimetre, not a micron: the local/global
            // round trip goes through a matrix inverse in f32 and is worth a couple of millimetres,
            // where a helper welded to the spinning deck lands tens of centimetres out.
            for globals in &rows {
                for (helper, joint) in helpers {
                    let off = globals[helper]
                        .w_axis
                        .truncate()
                        .distance(globals[joint].w_axis.truncate());
                    assert!(
                        off < 0.01,
                        "rung {rung}: {} is {off:.4} m from {}, so it is riding the deck",
                        frames.bone_names[helper],
                        frames.bone_names[joint],
                    );
                }
            }
            // And the discriminating half: the helpers must travel far less than the deck does. The
            // body is held still, so they barely move; welded to the deck they would move with it.
            let travel = |bone: usize| {
                let first = rows[0][bone].w_axis.truncate();
                rows.iter()
                    .map(|g| g[bone].w_axis.truncate().distance(first))
                    .fold(0f32, f32::max)
            };
            let deck = travel(truck);
            assert!(deck > 0.05, "rung {rung}: the deck hardly moves ({deck:.3} m)");
            for (helper, _) in helpers {
                let moved = travel(helper);
                assert!(
                    moved < 0.25 * deck,
                    "rung {rung}: {} travels {moved:.3} m against the deck's {deck:.3} -- it is \
                     following the board",
                    frames.bone_names[helper],
                );
            }
            assert_eq!(
                composed.frames.len(),
                frames.clip("360FLIP_D_HIGH_A").unwrap().frames.len(),
                "rung {rung}: the baked clip must keep the stock frame count, or the deck freezes \
                 or jumps part-way through the rung"
            );
        }
    }

    /// The deck actually turns, by a near-constant amount every frame, and keeps turning across rungs.
    ///
    /// This is the smoothness claim, measured on the baked output rather than on the estimator: a
    /// rung that re-accelerates, stalls or jumps fails here. The tolerance is loose on purpose --
    /// the comparison is against the mean step, and float noise through the local/global round trip
    /// is worth a fraction of a degree.
    #[test]
    #[ignore = "requires private stock animation banks"]
    fn the_held_deck_turns_at_a_steady_rate_and_carries_across_rungs() {
        let frames = banks();
        let bind = &frames.named_pose("RIG_TPOSE").unwrap().samples;
        let board = frames
            .bone_names
            .iter()
            .position(|n| n == "SKATEBOARD_ROOT")
            .unwrap();
        let hold = crate::graph_host::endless_flip::HOLD_BODY;
        let deck = |rung: u32| -> Vec<Quat> {
            hold_body(&frames, "360FLIP_D_HIGH_A", hold, rung)
                .unwrap()
                .frames
                .iter()
                .map(|row| row_globals(row, &frames, bind)[board].to_scale_rotation_translation().1)
                .collect()
        };
        let first = deck(2);
        let steps: Vec<f32> = first
            .windows(2)
            .map(|p| world_delta(p[0], p[1]).length())
            .collect();
        let mean = steps.iter().sum::<f32>() / steps.len() as f32;
        assert!(
            mean > 1f32.to_radians(),
            "the held deck barely turns: {:.3} deg per frame",
            mean.to_degrees()
        );
        for (i, step) in steps.iter().enumerate() {
            assert!(
                (step - mean).abs() < 0.05 * mean,
                "frame {i} turns {:.3} deg where the rung averages {:.3} -- the spin is not steady",
                step.to_degrees(),
                mean.to_degrees()
            );
        }
        // Across the junction: the next rung must continue, a touch slower, not restart.
        let second = deck(3);
        let junction = world_delta(*first.last().unwrap(), second[0]).length();
        assert!(
            (junction - mean).abs() < 0.2 * mean,
            "the rung junction steps {:.3} deg where the rung steps {:.3}",
            junction.to_degrees(),
            mean.to_degrees()
        );
        let next: Vec<f32> = second
            .windows(2)
            .map(|p| world_delta(p[0], p[1]).length())
            .collect();
        let next_mean = next.iter().sum::<f32>() / next.len() as f32;
        assert!(
            next_mean < mean && next_mean > 0.8 * mean,
            "rung 3 turns at {:.3} deg against rung 2's {:.3} -- expected slightly slower",
            next_mean.to_degrees(),
            mean.to_degrees()
        );
    }

    #[test]
    #[ignore = "requires private stock banks and the captured crouch/treflip asset"]
    fn authored_clip_rejects_bad_timing_and_skeleton() {
        let (frames, text) = fixture();
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["bone_names"][1] = "WRONG".into();
        assert!(Replacements::parse(&v.to_string(), &frames).is_err());
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["clips"]["360FLIP_D_HIGH_G"]["fps"] = 24.into();
        assert!(Replacements::parse(&v.to_string(), &frames).is_err());
    }
}
