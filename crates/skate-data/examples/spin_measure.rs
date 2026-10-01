//! What the authored board channel actually does, measured rather than inferred.
//!
//! The endless 360-flip hold synthesises a rigid board spin, and three of its choices have to come
//! from the clip instead of from a guess:
//!
//! 1. **Is the deck's rotation one fixed axis, or two?** A 360 flip is a kickflip (about the deck's
//!    long axis) plus a 360 shuvit (about world up). *Both* models predict a ~509-degree path, so
//!    the 494 degrees the existing `SKATE_SPIN` trace reports cannot tell them apart. The number
//!    that can is `|sum r| / sum |r|` over the per-frame rotation vectors: a genuinely fixed axis
//!    gives ~1.0, while a matched flip-plus-shuvit gives ~0.707, because the yawing flip component
//!    integrates to zero over a whole revolution. That matters enormously, because if it is 0.707
//!    then taking the axis as `normalize(sum r)` returns **world vertical** and the synthesised hold
//!    becomes a pure shuvit with the flip silently gone.
//! 2. **Which axis, and which direction.** Deriving it from the net first-to-last-frame rotation is
//!    what made the board spin the wrong way: a 494-degree path is not recoverable from its
//!    endpoints, and `to_axis_angle`'s sign then depends on an unrelated float branch.
//! 3. **Which point does it turn about.** The spin is applied about `SKATEBOARD_ROOT`'s origin. If
//!    that origin is not near the deck's centre, the synthesised spin orbits the wrong point.
//!
//! This lives in `skate-data` on purpose. `skate-core` has no dependencies and `skate-data` only
//! adds the loaders, so this builds in seconds -- where the same measurement inside `skate-game`
//! costs a multi-minute build and `RUST_MIN_STACK`. The quaternion arithmetic is hand-rolled for the
//! same reason, which also keeps the result independent of bevy's glam version.
//!
//! Run: `cargo run -p skate-data --example spin_measure -- <asset root>`

use skate_data::animation_banks::AnimationBanks;
use skate_data::animation_frames::{AnimationFrames, SampleWords};

type Quat = [f32; 4]; // x, y, z, w
type Vec3 = [f32; 3];

fn cross3(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Hamilton product, matching `skate_core::animation::pose_trajectory::multiply`.
fn mul(a: Quat, b: Quat) -> Quat {
    let v = cross3([a[0], a[1], a[2]], [b[0], b[1], b[2]]);
    [
        a[0] * b[3] + b[0] * a[3] + v[0],
        a[1] * b[3] + b[1] * a[3] + v[1],
        a[2] * b[3] + b[2] * a[3] + v[2],
        a[3] * b[3] - (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]),
    ]
}

fn conj(q: Quat) -> Quat {
    [-q[0], -q[1], -q[2], q[3]]
}

fn norm(q: Quat) -> Quat {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l <= f32::EPSILON {
        [0., 0., 0., 1.]
    } else {
        [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
    }
}

fn qdot(a: Quat, b: Quat) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// Rotate a vector by a quaternion, matching `pose_trajectory::rotate`.
fn rot(q: Quat, v: Vec3) -> Vec3 {
    let qv = [q[0], q[1], q[2]];
    let first = cross3(qv, v);
    let inter = [
        q[3] * v[0] + first[0],
        q[3] * v[1] + first[1],
        q[3] * v[2] + first[2],
    ];
    let second = cross3(qv, inter);
    [
        2.0 * second[0] + v[0],
        2.0 * second[1] + v[1],
        2.0 * second[2] + v[2],
    ]
}

/// Axis * angle: the rotation vector. Summing these cannot invert, where averaging bare axes can.
fn rotation_vector(q: Quat) -> Vec3 {
    let q = norm(q);
    // Canonicalise onto the short way round, so a delta never comes back as its 360-complement.
    let q = if q[3] < 0.0 {
        [-q[0], -q[1], -q[2], -q[3]]
    } else {
        q
    };
    let sin_half = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
    if sin_half <= 1e-9 {
        return [0., 0., 0.];
    }
    let angle = 2.0 * sin_half.atan2(q[3]);
    [
        q[0] / sin_half * angle,
        q[1] / sin_half * angle,
        q[2] / sin_half * angle,
    ]
}

fn length(v: Vec3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn unit(v: Vec3) -> Vec3 {
    let l = length(v);
    if l <= 1e-9 {
        [0., 0., 0.]
    } else {
        [v[0] / l, v[1] / l, v[2] / l]
    }
}

fn add3(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn dot3(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sqt(w: SampleWords) -> ([f32; 3], Quat, Vec3) {
    let f = |i: usize| f32::from_bits(w[i]);
    (
        [f(0), f(1), f(2)],
        [f(3), f(4), f(5), f(6)],
        [f(7), f(8), f(9)],
    )
}

/// One row's bone locals -> globals, as (rotation, origin, scale).
///
/// Mirrors `authored_clips::row_globals`: the local is `pose_add::add(row, bind, true)`, whose
/// rotation is `bind * row` and whose translation is `rotate(bind, row.t) + bind.t`. Bone 0
/// (TRAJECTORY) is left at identity, exactly as the walk there does.
fn row_globals(
    row: &[SampleWords],
    bind: &[SampleWords],
    parents: &[i32],
) -> Vec<(Quat, Vec3, [f32; 3])> {
    let mut out = vec![([0., 0., 0., 1.0f32], [0f32; 3], [1f32; 3]); row.len()];
    for bone in 1..row.len() {
        let (rs, rq, rt) = sqt(row[bone]);
        let (bs, bq, bt) = sqt(bind[bone]);
        let local_rot = mul(bq, rq);
        let rotated = rot(bq, rt);
        let local_pos = add3(rotated, bt);
        let local_scale = [bs[0] * rs[0], bs[1] * rs[1], bs[2] * rs[2]];
        let parent = parents[bone];
        if parent > 0 {
            let (pq, pp, ps) = out[parent as usize];
            let scaled = [
                local_pos[0] * ps[0],
                local_pos[1] * ps[1],
                local_pos[2] * ps[2],
            ];
            let moved = rot(pq, scaled);
            out[bone] = (
                norm(mul(pq, local_rot)),
                add3(pp, moved),
                [
                    ps[0] * local_scale[0],
                    ps[1] * local_scale[1],
                    ps[2] * local_scale[2],
                ],
            );
        } else {
            out[bone] = (norm(local_rot), local_pos, local_scale);
        }
    }
    out
}

/// World-space per-frame rotation vectors for one bone: `q_next * q_prev^-1`, sign-canonicalised.
fn deltas(globals: &[Vec<(Quat, Vec3, [f32; 3])>], bone: usize) -> Vec<Vec3> {
    globals
        .windows(2)
        .map(|pair| {
            let a = norm(pair[0][bone].0);
            let b = norm(pair[1][bone].0);
            let b = if qdot(a, b) < 0.0 {
                [-b[0], -b[1], -b[2], -b[3]]
            } else {
                b
            };
            rotation_vector(mul(b, conj(a)))
        })
        .collect()
}

/// Rotation about an axis by an angle, as a quaternion.
fn axis_angle(axis: Vec3, angle: f32) -> Quat {
    let a = unit(axis);
    let h = angle * 0.5;
    let s = h.sin();
    [a[0] * s, a[1] * s, a[2] * s, h.cos()]
}

/// The two-axis decomposition: world-up yaw (the shuvit) composed with a fixed board-local spin
/// (the flip), which is what a 360 flip actually is.
///
/// `Q_i = R_up(theta_i) * Q_0 * R_e(phi_i)`. The yaw is pulled out first as the world-up component
/// of each delta, which is a signed scalar and so cannot invert; what remains is a rotation about a
/// single axis in the deck's own frame, and *that* axis is well conditioned where the combined one
/// is not. Returns (per-frame yaw, per-frame flip, flip axis, flip axis stability).
fn decompose(
    globals: &[Vec<(Quat, Vec3, [f32; 3])>],
    bone: usize,
    up: Vec3,
) -> (Vec<f32>, Vec<f32>, Vec3, f32) {
    let d = deltas(globals, bone);
    let mut yaw = Vec::with_capacity(d.len());
    let mut theta = 0f32;
    let mut yawless = Vec::with_capacity(globals.len());
    for i in 0..globals.len() {
        yawless.push(mul(axis_angle(up, -theta), norm(globals[i][bone].0)));
        if i < d.len() {
            let step = dot3(d[i], unit(up));
            yaw.push(step);
            theta += step;
        }
    }
    // Body-frame deltas of the yaw-free sequence: P_{i+1} = P_i * R_e(dphi).
    let mut sum = [0f32; 3];
    let mut path = 0f32;
    let mut raw: Vec<Vec3> = Vec::with_capacity(d.len());
    for i in 0..d.len() {
        let a = norm(yawless[i]);
        let b = norm(yawless[i + 1]);
        let b = if qdot(a, b) < 0.0 { [-b[0], -b[1], -b[2], -b[3]] } else { b };
        let r = rotation_vector(mul(conj(a), b));
        path += length(r);
        sum = add3(sum, r);
        raw.push(r);
    }
    let axis = unit(sum);
    let stability = if path > 1e-6 { length(sum) / path } else { 0.0 };
    let flip = raw.iter().map(|r| dot3(*r, axis)).collect();
    (yaw, flip, axis, stability)
}

fn index(frames: &AnimationFrames, name: &str) -> Option<usize> {
    frames.bone_names.iter().position(|n| n == name)
}

fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).expect("asset root"));
    let banks = AnimationBanks::load(&root).unwrap();
    let frames = AnimationFrames::from_banks(&banks).unwrap();
    let bind = &frames.named_pose("RIG_TPOSE").unwrap().samples;

    let board = index(&frames, "SKATEBOARD_ROOT").expect("SKATEBOARD_ROOT");
    let first_helper = [
        "RIGHTTOEBASE_REPARENTED",
        "LEFTTOEBASE_REPARENTED",
        "RIGHTHAND_REPARENTED",
        "LEFTHAND_REPARENTED",
    ]
    .iter()
    .filter_map(|n| index(&frames, n))
    .min()
    .expect("reparented helpers");
    println!(
        "bones={}  board={board} ({})  board subtree={board}..{first_helper}  helpers from {first_helper}",
        frames.bone_names.len(),
        frames.bone_names[board],
    );
    for bone in board..first_helper {
        println!("    {bone:>3} {} parent={}", frames.bone_names[bone], frames.parents[bone]);
    }
    println!();

    // Discovered from the bank, not guessed: the tree/bank name gap is what put a hold pose on a
    // repeat in the first place.
    let wanted = [
        "360FLIP",
        "360HARDFLIP",
        "360INWARDHEELFLIP",
        "360POPSHUVIT",
        "FS360POPSHUVIT",
        "LASERFLIP",
    ];
    let mut names: Vec<String> = frames
        .clip_names()
        .filter(|n| n.ends_with("_A"))
        .filter(|n| {
            let stem = n.trim_start_matches("N_").trim_start_matches("NOLLIE_");
            wanted.iter().any(|w| stem.starts_with(w))
        })
        .map(str::to_owned)
        .collect();
    names.sort();
    if names.is_empty() {
        println!("No air clip matched. Everything with 360FLIP in the name:");
        let mut all: Vec<&str> = frames.clip_names().filter(|n| n.contains("360FLIP")).collect();
        all.sort();
        for n in all.iter().take(60) {
            println!("  {n}");
        }
        return;
    }

    for name in &names {
        let clip = match frames.clip(name) {
            Ok(c) => c,
            Err(e) => {
                println!("{name}: {e}");
                continue;
            }
        };
        if clip.frames.len() < 3 {
            println!("{name}: only {} frames", clip.frames.len());
            continue;
        }
        let fps = f32::from_bits(clip.fps_bits);
        let globals: Vec<Vec<(Quat, Vec3, [f32; 3])>> = clip
            .frames
            .iter()
            .map(|row| row_globals(row, bind, &frames.parents))
            .collect();

        println!("=== {name}   frames={} fps={fps:.2}", clip.frames.len());

        // ---- 1. Per board-subtree bone: the path (what SKATE_SPIN reports) and the vector sum.
        //
        // Equal paths across the trucks and wheels is a *tautology* of rigidity, so it confirms the
        // deck is one body but says nothing about whether the axis is fixed. The ratio does.
        let mut best = (board, 0f32);
        for bone in board..first_helper {
            let d = deltas(&globals, bone);
            let path: f32 = d.iter().map(|r| length(*r)).sum();
            let sum = d.iter().fold([0f32; 3], |a, r| add3(a, *r));
            let ratio = if path > 1e-6 { length(sum) / path } else { 0.0 };
            println!(
                "  {:<22} path={:7.1}deg  |sum|={:7.1}deg  ratio={ratio:.3}  axis=[{:6.3} {:6.3} {:6.3}]",
                frames.bone_names[bone],
                path.to_degrees(),
                length(sum).to_degrees(),
                unit(sum)[0],
                unit(sum)[1],
                unit(sum)[2],
            );
            if path > best.1 {
                best = (bone, path);
            }
        }
        let bone = best.0;
        let d = deltas(&globals, bone);
        let path: f32 = d.iter().map(|r| length(*r)).sum();
        let sum = d.iter().fold([0f32; 3], |a, r| add3(a, *r));
        let ratio = if path > 1e-6 { length(sum) / path } else { 0.0 };
        println!(
            "  -> most rotation: {} ({:.1}deg)",
            frames.bone_names[bone],
            path.to_degrees()
        );

        // ---- 2. THE DECISION: one fixed axis, or a flip plus a shuvit?
        println!(
            "  VERDICT ratio={ratio:.3}  =>  {}",
            if ratio > 0.92 {
                "ONE FIXED AXIS -- normalize(sum r) is the spin axis"
            } else if ratio < 0.80 {
                "TWO AXES (flip + shuvit) -- a single axis would drop the flip; use two scalars"
            } else {
                "AMBIGUOUS -- inspect the deck-local breakdown below"
            }
        );

        // ---- 3. Deck-local breakdown: which of the deck's own axes carries the flip.
        //
        // `Q_i^-1 r_i` re-expresses each world delta in the deck's frame, so a component that
        // survives the sum is a rotation about a *board* axis (the flip), where the yaw component
        // cancels as the deck turns.
        let mut local_sum = [0f32; 3];
        for (i, r) in d.iter().enumerate() {
            let q = norm(globals[i][bone].0);
            local_sum = add3(local_sum, rot(conj(q), *r));
        }
        println!(
            "  deck-local sum=[{:7.1} {:7.1} {:7.1}]deg  |{:.1}|deg",
            local_sum[0].to_degrees(),
            local_sum[1].to_degrees(),
            local_sum[2].to_degrees(),
            length(local_sum).to_degrees(),
        );

        // ---- 4. Bug A, side by side: the endpoint axis the code uses against the accumulated one.
        let a = norm(globals[0][bone].0);
        let b = norm(globals[globals.len() - 1][bone].0);
        let net = rotation_vector(mul(b, conj(a)));
        let net_axis = unit(net);
        let accum_axis = unit(sum);
        let agreement = dot3(net_axis, accum_axis);
        println!(
            "  endpoint: axis=[{:6.3} {:6.3} {:6.3}] angle={:6.1}deg   <- what the code uses today",
            net_axis[0],
            net_axis[1],
            net_axis[2],
            length(net).to_degrees()
        );
        println!(
            "  accumul.: axis=[{:6.3} {:6.3} {:6.3}] path ={:6.1}deg   dot={agreement:+.3}{}",
            accum_axis[0],
            accum_axis[1],
            accum_axis[2],
            path.to_degrees(),
            if agreement < 0.0 { "  <== INVERTED" } else { "" }
        );

        // ---- 5. The speed profile, so the catch's deceleration is a number not an impression.
        let speeds: Vec<f32> = d.iter().map(|r| length(*r)).collect();
        print!("  deg/frame:");
        for (i, s) in speeds.iter().enumerate() {
            if i % 12 == 0 {
                print!("\n   {i:>3}: ");
            }
            print!("{:>7.1}", s.to_degrees());
        }
        println!();

        // ---- 6. The plateau rate, robustly: the mean of the fastest third of frames. Peak-picking
        // lands on the flick spike, which is the fastest moment of the clip and not the speed the
        // rotation settles at.
        let mut sorted = speeds.clone();
        sorted.sort_by(f32::total_cmp);
        let third = (sorted.len() / 3).max(1);
        let plateau: f32 = sorted[sorted.len() - third..].iter().sum::<f32>() / third as f32;
        let whole: f32 = speeds.iter().sum::<f32>() / speeds.len() as f32;
        // Last frame still at plateau speed = the handoff, just before the catch slows the deck.
        let handoff = speeds
            .iter()
            .rposition(|&s| s >= 0.9 * plateau)
            .unwrap_or(speeds.len() - 1);
        println!(
            "  plateau={:.2}deg/frame  whole-clip mean={:.2}  ratio={:.2}  handoff frame={handoff}/{}",
            plateau.to_degrees(),
            whole.to_degrees(),
            whole / plateau,
            speeds.len()
        );
        println!(
            "  one rung of {} frames at the plateau sweeps {:.0}deg   (clip path {:.0}deg)",
            clip.frames.len(),
            (plateau * clip.frames.len() as f32).to_degrees(),
            path.to_degrees()
        );

        // ---- 7. The pivot. The spin turns the deck about SKATEBOARD_ROOT's origin; if that is not
        // near the midpoint of the trucks the synthesised spin orbits the wrong point.
        let origin = |bone: usize| globals[handoff.min(globals.len() - 1)][bone].1;
        let root = origin(board);
        let trucks: Vec<(usize, Vec3)> = (board..first_helper)
            .filter(|&b| frames.bone_names[b].contains("TRUCK"))
            .map(|b| (b, origin(b)))
            .collect();
        print!(
            "  pivot: root=[{:6.3} {:6.3} {:6.3}]",
            root[0], root[1], root[2]
        );
        if trucks.len() == 2 {
            let mid = [
                (trucks[0].1[0] + trucks[1].1[0]) * 0.5,
                (trucks[0].1[1] + trucks[1].1[1]) * 0.5,
                (trucks[0].1[2] + trucks[1].1[2]) * 0.5,
            ];
            let off = length([mid[0] - root[0], mid[1] - root[1], mid[2] - root[2]]);
            print!(
                "  truck mid=[{:6.3} {:6.3} {:6.3}]  offset={off:.3}m{}",
                mid[0],
                mid[1],
                mid[2],
                if off > 0.08 { "  <== root is not the deck centre" } else { "" }
            );
        }

        // ---- 8. The two-axis model, and whether it reproduces the authored clip.
        //
        // This is the acid test. If `R_up(theta) * Q_anchor * R_e(phi)` reconstructs every authored
        // frame to within a couple of degrees, then the model *is* the trick and a hold built on it
        // keeps the flip and the shuvit both. A single fixed axis cannot: the sum above points at
        // world up, so it would synthesise a pure shuvit.
        let up = [0.0, 1.0, 0.0];
        let (yaw, flip, flip_axis, stability) = decompose(&globals, bone, up);
        let total_yaw: f32 = yaw.iter().sum();
        let total_flip: f32 = flip.iter().sum();
        println!(
            "  TWO-AXIS  yaw(up)={:7.1}deg  flip={:7.1}deg about deck axis=[{:6.3} {:6.3} {:6.3}] stability={stability:.3}",
            total_yaw.to_degrees(),
            total_flip.to_degrees(),
            flip_axis[0], flip_axis[1], flip_axis[2],
        );
        // Reconstruct and report the worst frame error.
        let mut worst = 0f32;
        let mut theta = 0f32;
        let mut phi = 0f32;
        for i in 0..globals.len() {
            let model = mul(mul(axis_angle(up, theta), norm(globals[0][bone].0)), axis_angle(flip_axis, phi));
            let actual = norm(globals[i][bone].0);
            let m = if qdot(model, actual) < 0.0 { [-model[0], -model[1], -model[2], -model[3]] } else { model };
            worst = worst.max(length(rotation_vector(mul(actual, conj(m)))));
            if i < yaw.len() {
                theta += yaw[i];
                phi += flip[i];
            }
        }
        println!(
            "  reconstruction worst error={:.2}deg  =>  {}",
            worst.to_degrees(),
            if worst.to_degrees() < 8.0 {
                "MODEL FITS -- synthesise yaw + flip"
            } else {
                "model does not fit -- do not build on it"
            }
        );
        let yplateau = {
            let mut v: Vec<f32> = yaw.iter().map(|x| x.abs()).collect();
            v.sort_by(f32::total_cmp);
            let t = (v.len() / 3).max(1);
            v[v.len() - t..].iter().sum::<f32>() / t as f32
        };
        let fplateau = {
            let mut v: Vec<f32> = flip.iter().map(|x| x.abs()).collect();
            v.sort_by(f32::total_cmp);
            let t = (v.len() / 3).max(1);
            v[v.len() - t..].iter().sum::<f32>() / t as f32
        };
        println!(
            "  plateau rates: yaw={:.2}deg/frame flip={:.2}deg/frame  (signs: yaw {} flip {})",
            yplateau.to_degrees(), fplateau.to_degrees(),
            if total_yaw >= 0.0 { "+" } else { "-" },
            if total_flip >= 0.0 { "+" } else { "-" },
        );

        // ---- 9. DELTA REPLAY: the model that needs no axis at all.
        //
        // Neither a single fixed axis nor a yaw+flip decomposition reconstructs this clip. But the
        // complaint was never about the *shape* of the authored rotation -- it is the authored trick,
        // and it is what the owner wants continued. The complaint is that replaying the clip replays
        // its **acceleration**: a ramp in, a plateau, then the catch slowing the deck.
        //
        // So keep the authored angular velocity *directions* and flatten only their magnitudes to the
        // plateau rate, then compose them incrementally. Composing deltas rather than setting
        // orientations means the deck's orientation is continuous by construction -- there is no loop
        // to close, no window to trim and no seam to cross-fade, which is what the four resampling
        // attempts each broke on. And the direction is inherited from the clip, so it cannot invert.
        // Whole clip, no window detection: keep every authored direction, flatten every magnitude
        // to the plateau rate. Window detection proved unstable -- some variants are near-uniform and
        // the detector collapsed to three frames -- and it is not needed, because flattening the
        // magnitudes is what removes the ramp and the catch.
        let flatten = |dirs: &[Vec3]| -> Vec<Vec3> {
            dirs.iter()
                .map(|r| {
                    let u = unit(*r);
                    [u[0] * plateau, u[1] * plateau, u[2] * plateau]
                })
                .collect()
        };
        // A 3-tap mean over the direction sequence, which is what "smooth" has to mean concretely.
        let smooth = |dirs: &[Vec3]| -> Vec<Vec3> {
            (0..dirs.len())
                .map(|i| {
                    let a = dirs[if i == 0 { dirs.len() - 1 } else { i - 1 }];
                    let b = dirs[i];
                    let c = dirs[(i + 1) % dirs.len()];
                    unit(add3(add3(unit(a), unit(b)), unit(c)))
                })
                .collect()
        };
        let report = |label: &str, w: &[Vec3]| {
            let mut worst_turn = 0f32;
            for pair in w.windows(2) {
                worst_turn = worst_turn.max(dot3(unit(pair[0]), unit(pair[1])).clamp(-1., 1.).acos());
            }
            // Wrap-around turn too: this is the junction between one cycle and the next.
            let wrap = dot3(unit(w[w.len() - 1]), unit(w[0])).clamp(-1., 1.).acos();
            let mut q = [0., 0., 0., 1.0f32];
            for r in w {
                q = norm(mul(axis_angle(*r, length(*r)), q));
            }
            println!(
                "  {label:<22} frames={:>3} path/cycle={:>5.0}deg  worst turn={:>5.1}deg  wrap turn={:>5.1}deg  precession={:>4.0}deg",
                w.len(),
                w.iter().map(|r| length(*r)).sum::<f32>().to_degrees(),
                worst_turn.to_degrees(),
                wrap.to_degrees(),
                length(rotation_vector(q)).to_degrees(),
            );
        };
        println!("  DELTA REPLAY (compose per-frame deltas; orientation continuous by construction)");
        report("authored dirs", &flatten(&d));
        report("3-tap smoothed", &flatten(&smooth(&d)));
        report("5-tap smoothed", &flatten(&smooth(&smooth(&d))));
        println!(
            "  one rung of {} frames at the flattened rate sweeps {:.0}deg of path   (authored clip {:.0}deg)",
            clip.frames.len(),
            (plateau * clip.frames.len() as f32).to_degrees(),
            path.to_degrees()
        );
        println!();
        println!();
    }
}
