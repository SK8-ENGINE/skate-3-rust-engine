//! The stock held-flip ladder and the air late flips.
//!
//! `T_Kickflip.xml` turns one authored flip into `<Trick>2`/`3`/`4` by cycling
//! `B_<TRICK>_CYC1..3`, but only while the scoop coordinate is still held *and*
//! the takeoff trajectory has the authored time left: `End.Out.Out1` claims the
//! exit at `TimeToLand < 0.525` and `Out2` at `< 0.8`, and those exits are
//! listed before the cycle transitions, so a short pop ends the ladder early.
//! `KnownAir` takes its prediction once, on entering the air, so the pop alone
//! decides how far the ladder can run. That is why these replays pop hard
//! instead of editing the world: the canonical course carries authored query
//! metadata that a rebuilt `BoardWorld` would drop.
//!
//! Every input here is a real controller sample fed through the production
//! gesture recognizer, action graph, motion graph and physics.
use super::*;
use skate_core::animation::{output::attributes::AttributeName, skeleton_input::name::encode};

/// Pop, hold the scoop, and the graph must walk the whole authored ladder.
/// The heelflip's authored clips are longer than the kickflip's, so it needs a
/// taller pop to reach the same count -- that is the authored gate, not a bug.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn held_flips_cycle_through_the_authored_double_triple_and_quad() {
    for (trick, boost) in [("Kickflip", 8.0), ("Heelflip", 10.0)] {
        let run = replay(Input::Held { trick, boost }, 260);
        for count in ["", "2", "3", "4"] {
            assert!(
                run.tricks.contains(&format!("{trick}{count}")),
                "{trick}: held ladder never published {trick}{count}; saw {:?} over {:?}",
                run.tricks,
                run.animations
            );
        }
        let cycles = run
            .animations
            .iter()
            .filter(|a| a.ends_with("_CYC1") || a.ends_with("_CYC2") || a.ends_with("_CYC3"))
            .count();
        assert_eq!(
            cycles, 3,
            "{trick}: expected the three authored cycle clips, saw {:?}",
            run.animations
        );
    }
}

/// The ladder, landed: the quad has to reach the scorer as the quad's own
/// scorable and be named by the quad's authored label, not the single's.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn landed_quads_bank_and_name_the_quad_scorable() {
    for (trick, boost, label) in [
        ("Kickflip", 8.0, "#ID_TRICK_FLIP_QUADRUPLE_KICKFLIP"),
        ("Heelflip", 10.0, "#ID_TRICK_FLIP_QUADRUPLE_HEELFLIP"),
    ] {
        let run = replay(Input::Held { trick, boost }, 300);
        assert!(
            run.tricks.contains(&format!("{trick}4")),
            "{trick}: the ladder no longer reaches the quad: {:?} over {:?}",
            run.tricks,
            run.animations
        );
        assert_eq!(run.name, label, "{trick}: the landed quad published");
        // The ladder converts rather than accumulating, so only the quad's own
        // authored 250 is banked -- never the single's 100 as well.
        assert!(
            run.reward > 250.,
            "{trick}: the landed quad banked {}",
            run.reward
        );
    }
}

/// The single flip is the baseline the ladder must not fall below. The ladder
/// *converts* its carrier (`LINKS`), so each count replaces the previous one's
/// reward instead of adding to it -- a regression there would silently make a
/// quad worth less than a single.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn a_single_flip_is_the_ladder_baseline() {
    let single = replay(
        Input::Single {
            trick: "Kickflip",
            boost: 8.0,
        },
        300,
    );
    let quad = replay(
        Input::Held {
            trick: "Kickflip",
            boost: 8.0,
        },
        300,
    );
    eprintln!("single={} quad={}", single.reward, quad.reward);
    assert_eq!(
        single.tricks,
        ["Kickflip"],
        "the released flick still cycled"
    );
    assert!(
        quad.reward >= single.reward,
        "the quad banked {} but the single banked {}",
        quad.reward,
        single.reward
    );
}

/// An ollie, then the air scoop: `air.xml`'s `Lateflip` state, whose leaves name
/// `latekickflip`/`lateheelflip` and friends. These gestures live in their own
/// recognizer (`skater_air.pat`) and never enter the 270-row trick mapping, so
/// they reach the motion graph as bare intents.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn air_scoops_reach_the_authored_late_flips() {
    for (gesture, trick, label) in [
        (
            "L_F_Kickflip",
            "LateKickflip",
            "#ID_TRICK_FLIP_LATE_KICKFLIP",
        ),
        (
            "L_F_Heelflip",
            "LateHeelflip",
            "#ID_TRICK_FLIP_LATE_HEELFLIP",
        ),
    ] {
        let run = replay(Input::Late { gesture, trick }, 260);
        assert!(
            run.tricks.contains(&trick.to_owned()),
            "{gesture}: the air scoop never published {trick}; saw {:?} over {:?}",
            run.tricks,
            run.animations
        );
        assert_eq!(run.name, label, "{gesture}: the landed late flip published");
    }
}

/// A held rotation keeps the board turning while the body stays put. That is the whole contract.
///
/// **This test used to assert the opposite** -- that a repeat swaps in the authored
/// `T_<trick>_H_CYC` cycle clip -- and that was wrong twice over. Those clips are hold *poses*, not
/// rotations: measured, they move the board a tenth as much as the kickflip's cycle does, so playing
/// one on a repeat parked the deck and only the first rotation was ever visible. And the assertion
/// watched `animations`, which holds the *tree* names the graph plays, where the hold is composed at
/// bank level and never appears there at all.
///
/// So the rung keeps playing the trick's own air clip -- which is what turns the deck -- and
/// `animation_pose` replaces the body alone. The deck must not be parked, which is the failure this
/// replaced and the one thing this sim-level run can decide.
///
/// **It cannot decide the other half.** `body_motion` sums *local* rotations, and the four
/// reparented helpers are children of the board, so holding them still in world space demands
/// locals that counter-rotate the deck -- the faster the deck turns, the more "body movement" the
/// metric reports. Measured: turning the synthesis off moves the 360 flip from 2.70 to 2.23 while
/// the board goes 0.54 to 0.28, so most of that difference is the metric watching the deck through
/// the helpers. The body actually staying put is asserted where it can be, directly on the composed
/// clip, in `authored_clips`' `the_held_hands_and_toes_follow_the_body_not_the_spun_deck`.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn a_held_rotation_turns_the_board_and_not_the_body() {
    for trick in ["360Flip", "Laserflip"] {
        let stock = replay(Input::Held { trick, boost: 14.0 }, 600);
        let endless = replay(
            Input::EndlessCapped { trick, boost: 14.0, extra: 2 },
            600,
        );
        assert!(
            endless.tricks.len() > stock.tricks.len(),
            "{trick}: the loop never ran"
        );
        assert!(
            endless.motion_ticks > 0 && stock.motion_ticks > 0,
            "{trick}: nothing was sampled to compare"
        );
        let per = |total: f32, ticks: u32| total / ticks as f32;
        // Per tick, over the same clip the same gate selects, so this is a matched comparison of the
        // one channel that matters: the deck must keep turning at least as fast as the authored
        // trick turns it. The synthesis runs at the measured plateau, which is faster.
        let board = per(endless.board_motion, endless.motion_ticks);
        let stock_board = per(stock.board_motion, stock.motion_ticks);
        assert!(
            board >= 0.9 * stock_board,
            "{trick}: the held board moves {board} per tick against the authored {stock_board} -- \
             it is parked"
        );
    }
}

/// A rung has to cost a clip. If rungs outrun the air, the loop is re-firing on a finished clip.
///
/// This is the shape of a bug that reached play and that no smoothness measurement could see: the
/// rung counter raced while a single animation sat on screen, because a self-transition carrying
/// `transitionUnder` nests the incoming clip inside the live transition. Re-entering builds a
/// tower that never resolves, so `crossed_end` never clears and the condition fires every tick.
///
/// The budget is derived from the clip a rung actually plays, which is now the trick's own **air**
/// clip rather than a cycle clip: measured, `360FLIP_D_HIGH_A` is 33 frames and `_LOW_A` is 28, at
/// 60 fps against the 60 Hz tick. Half of the shorter one is a generous rung and still an order of
/// magnitude away from a per-tick race. (The old divisor of 30 came from the 61-frame cycle clip,
/// which a rung no longer plays.)
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn a_rung_costs_a_whole_cycle_clip() {
    let run = replay(Input::Endless { trick: "360Flip", boost: 14.0 }, 600);
    assert!(run.tricks.len() > 1, "the ladder never left the authored rung");
    let budget = run.air_ticks / 14;
    assert!(
        run.tricks.len() as u32 <= budget,
        "{} rungs in {} air ticks is faster than the clip can play (budget {budget})",
        run.tricks.len(),
        run.air_ticks
    );
}

/// Past the octuple the counted names run out and the trick takes the mod's own name.
///
/// The rung is pinned with the mod's own cap, so this measures the naming and nothing else --
/// anchoring it to a takeoff speed only measures how much air the test course happens to give.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn the_ninth_flip_and_beyond_is_named_endless() {
    let octuple = replay(
        Input::EndlessCapped {
            trick: "Kickflip",
            boost: 16.0,
            extra: 4,
        },
        700,
    );
    assert_eq!(octuple.tricks.len(), 8, "expected the counted ceiling");
    assert_eq!(octuple.name, "#OCTUPLE KICKFLIP");

    let ninth = replay(
        Input::EndlessCapped {
            trick: "Kickflip",
            boost: 16.0,
            extra: 5,
        },
        700,
    );
    assert_eq!(ninth.tricks.len(), 9, "expected exactly a ninth rung");
    assert_eq!(
        ninth.name, "#ENDLESS KICKFLIP",
        "the ninth rung should stop counting and take the mod's name"
    );
}

/// The loop junction must not be rougher than an authored one.
///
/// `_CYC3` is authored as the third clip of a sequence, so re-entering it cuts. Measured before
/// the fix, a skeleton part moved 0.41 m in a single tick at each junction against 0.24 m for the
/// worst authored one, once per cycle -- which read on screen as the arms snapping around. The
/// loop transition carries a `blendWithCurrentFrame` override so it cross-fades out of the pose
/// on screen instead. Comparing against the same pop with the feature off keeps this honest: the
/// takeoff is the roughest moment in any flip, and it is stock.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn the_loop_junction_is_no_rougher_than_an_authored_one() {
    let boost = 14.0;
    let worst_in_flip = |o: &Outcome| {
        o.jolts
            .iter()
            .filter(|(_, _, clip)| clip.contains("KICKFLIP"))
            .map(|(_, d, _)| *d)
            .fold(0f32, f32::max)
    };
    let stock = replay(Input::Held { trick: "Kickflip", boost }, 600);
    let endless = replay(Input::Endless { trick: "Kickflip", boost }, 600);
    assert!(endless.tricks.len() > stock.tricks.len(), "the loop never ran");

    let (a, b) = (worst_in_flip(&stock), worst_in_flip(&endless));
    assert!(
        b <= a + 0.01,
        "the endless junction is rougher than the authored worst: {b} against {a}"
    );
    let roughest_clip = endless
        .jolts
        .iter()
        .filter(|(_, _, clip)| clip.contains("KICKFLIP"))
        .max_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(_, _, clip)| clip.clone())
        .unwrap_or_default();
    assert!(
        !roughest_clip.contains("_CYC"),
        "a cycle clip is the roughest moment of the flip: {roughest_clip}"
    );
}

/// The same measurement for the 360 flip family, which until now nothing defended.
///
/// The test above filters on `KICKFLIP`, so it says nothing about the family whose hold is
/// synthesised -- and the 360-flip jolt figures in `docs/endless-tricks.md` were taken by hand
/// against a code path that no longer exists. This family is the harder case: retail authors no
/// cycle for it, so the deck's rotation through a held rung is composed rather than played, and a
/// discontinuity in that composition is exactly what the owner saw as the board jumping.
///
/// **The bar is not parity with the authored trick, because the ladder does not reach it.** Measured
/// on the flat course at this boost, a held 360 flip's worst single-tick skeleton jump is 0.61 m
/// against the authored trick's 0.23. That gap is pre-existing and is **not** the synthesised spin:
/// it is identical with `SKATE_ENDLESS_RIGID_SPIN=0`, and the jolt probe names the part as bone 3
/// rather than anything in the board subtree. Its two mechanisms, the hold arriving and the air clip
/// restarting from frame 0 each rung, are written up in `docs/engine-defects.md`.
///
/// So this guards against **regression** rather than asserting a bar the mechanism cannot meet. The
/// ceiling is the measured state plus a margin; the point is that the deck synthesis must not add to
/// it, which is the thing that would show as the board jumping.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn the_synthesised_spin_is_no_rougher_than_the_authored_trick() {
    for trick in ["360Flip", "Laserflip"] {
        let worst = |o: &Outcome, stem: &str| {
            o.jolts
                .iter()
                .filter(|(_, _, clip)| clip.to_uppercase().contains(stem))
                .map(|(_, d, _)| *d)
                .fold(0f32, f32::max)
        };
        let stem = trick.to_uppercase();
        let stock = replay(Input::Held { trick, boost: 14.0 }, 600);
        let endless = replay(
            Input::EndlessCapped { trick, boost: 14.0, extra: 2 },
            600,
        );
        assert!(
            endless.tricks.len() > stock.tricks.len(),
            "{trick}: the loop never ran"
        );
        let (a, b) = (worst(&stock, &stem), worst(&endless, &stem));
        assert!(a > 0.0, "{trick}: nothing was measured for {stem}");
        println!("JOLT {trick}: authored {a:.4} held {b:.4}");
        // Measured 0.606 (360 flip) and 0.474 (laserflip) with the ease in. 0.70 leaves room for
        // course and float variation while still catching anything that makes the deck jump.
        assert!(
            b <= 0.70,
            "{trick}: the held rung jolts {b} against a measured ceiling of 0.70 (authored {a})"
        );
        // And the deck must not be the part that jumps. This is the assertion that would fail if the
        // spin synthesis regressed -- the size alone could not tell the deck from the body.
        let worst_part = endless
            .jolt_parts
            .iter()
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .map_or(0, |(_, _, part)| *part);
        assert!(
            worst_part < 25,
            "{trick}: the roughest part is {worst_part}, which is in the board subtree -- the \
             synthesised deck is jumping"
        );
    }
}

/// The 360 flip family laddering, which is a different shape of problem from the kickflip.
///
/// `T_TrickWithDarkCatch.xml` authors no cycle *states* and `360flip`/`laserflip` have no numbered
/// scorables, so the rungs are synthesised. Retail also scopes the hold mechanic to the kickflip
/// and heelflip, so these gestures cannot be held at all unless the mod widens it. The rungs count
/// by board rotation: a doubled 360 flip is a 720.
///
/// The expectation is derived from the rungs actually reached rather than pinned to a boost. The
/// authored cycle clip is 61 frames against the air clip's 33, so a rotation takes about twice as
/// long as it used to and how many fit in a given pop is a property of the clip, not of the
/// naming this is here to check.
/// PROBE, not an assertion: what a held rung actually looks like. The hold the owner wants is
/// "the legs stay out of the way and the board rotates", which is exactly what the body/board
/// split measures -- near-zero body movement against a board that keeps turning.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn probe_what_a_held_rung_animates() {
    for trick in ["Kickflip", "360Flip", "Laserflip"] {
        let run = replay(Input::Endless { trick, boost: 14.0 }, 600);
        let per = |total: f32| {
            if run.motion_ticks == 0 {
                0.
            } else {
                total / run.motion_ticks as f32
            }
        };
        let mut clips: Vec<String> = Vec::new();
        for clip in &run.animations {
            if clips.last() != Some(clip) {
                clips.push(clip.clone());
            }
        }
        let stem = trick.to_uppercase();
        let mut ranked: Vec<&(u32, f32, String)> = run
            .jolts
            .iter()
            .filter(|(_, _, clip)| clip.to_uppercase().contains(&stem))
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let worst = ranked.first().map_or(0.0, |j| j.1);
        for (tick, size, clip) in ranked.iter().take(6) {
            let part = run
                .jolt_parts
                .iter()
                .find(|(t, _, _)| t == tick)
                .map_or(usize::MAX, |(_, _, p)| *p);
            println!("  JOLT {trick} tick={tick} {size:.4} part={part} in {clip}");
        }
        println!(
            "PROBE {trick}: rungs={} name={:?} body={:.4} board={:.4} ratio={:.2} jolt={:.4} ticks={} clips={:?}",
            run.tricks.len(),
            run.name,
            per(run.body_motion),
            per(run.board_motion),
            per(run.board_motion) / per(run.body_motion).max(1e-6),
            worst,
            run.motion_ticks,
            clips,
        );
    }
}

/// Where the stick has to finish, and stay, for each holdable family.
///
/// A hold is kept only while the stick stays within tolerance of the gesture's **final** authored
/// coordinate -- `gesture::Recognizer::held` compares against `points.last()`. So "I could not hold
/// anything but the kickflip and the 360 flip" has a concrete answer per trick, and it is in the PAT
/// data rather than in anybody's opinion. Printed as the coordinate to park the stick on.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn probe_where_to_hold_each_family() {
    let root = std::path::PathBuf::from(
        std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT"),
    );
    let compass = |[x, y]: [f32; 2]| {
        let vertical = if y > 0.35 { "up" } else if y < -0.35 { "down" } else { "" };
        let horizontal = if x > 0.35 { "right" } else if x < -0.35 { "left" } else { "" };
        match (vertical, horizontal) {
            ("", "") => "centre".to_owned(),
            (v, "") => v.to_owned(),
            ("", h) => h.to_owned(),
            (v, h) => format!("{v}-{h}"),
        }
    };
    for trick in [
        "Kickflip",
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
        let points = authored("skater.pat", &root, trick);
        match points.last() {
            Some(&last) => println!(
                "HOLD {trick:<22} points={} finish=[{:+.2} {:+.2}]  park the stick {}",
                points.len(),
                last[0],
                last[1],
                compass(last),
            ),
            None => println!("HOLD {trick:<22} no authored pattern"),
        }
    }
}

/// Does a rung past the authored quad cost the same time as a rung inside it?
///
/// Reported from play: "in kickflip endless the board speeds up once it goes past quad when it needs
/// to stay the same speed." The kickflip is a **cycle** ladder -- it replays the authored `Cyc2`/`Cyc3`
/// pair and never touches the synthesised spin -- so if it accelerates, the cause is the loop firing
/// sooner rather than the deck turning faster. Those two look identical on screen and are told apart
/// by the gap between rungs: a rung that costs a whole clip holds its spacing, and a rung that re-fires
/// early does not.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn probe_rung_spacing_across_the_quad() {
    for trick in ["Kickflip", "Heelflip", "360Flip"] {
        let run = replay(Input::Endless { trick, boost: 14.0 }, 600);
        let gaps: Vec<i64> = run
            .rung_ticks
            .windows(2)
            .map(|w| w[1].0 as i64 - w[0].0 as i64)
            .collect();
        println!(
            "SPACING {trick:<10} rungs={} ticks={:?}\n   gaps={:?}",
            run.rung_ticks.len(),
            run.rung_ticks.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
            gaps,
        );
        // Per-clip board speed, so a genuinely faster deck can be separated from faster rungs.
        let mut runs: Vec<(String, u32, f32, f32)> = Vec::new();
        for (clip, delta, length) in &run.board_per_clip {
            match runs.last_mut() {
                Some(last) if &last.0 == clip => {
                    last.1 += 1;
                    last.2 += delta;
                    last.3 = *length;
                }
                _ => runs.push((clip.clone(), 1, *delta, *length)),
            }
        }
        for (clip, ticks, total, length) in &runs {
            println!(
                "   {clip:<24} ticks={ticks:<4} board/tick={:.4} clock_length={length:.3}s ({:.1} ticks)",
                total / *ticks as f32,
                length * 60.0,
            );
        }
    }
}

/// Which of the twelve installed ladders actually run when the gesture *is* performed.
///
/// Reported from play: only the kickflip and the 360 flip could be held. Widening `HoldPattern` to
/// every family with a rung table was necessary but may not be sufficient, and the two causes look
/// identical from the sofa -- a family the engine will not ladder, against a gesture that is simply
/// hard to flick and hold. This harness replays the authored PAT coordinates, so it removes the stick
/// from the question entirely: whatever fails here is the engine's fault, and whatever passes here is
/// an input problem rather than a code one.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn probe_which_families_ladder() {
    for trick in [
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
        let stock = replay(Input::Held { trick, boost: 14.0 }, 600);
        let run = replay(
            Input::EndlessCapped { trick, boost: 14.0, extra: 2 },
            600,
        );
        println!(
            "LADDER {trick:<22} stock={} endless={} name={:?} reward={:.0} board={:.4}",
            stock.tricks.len(),
            run.tricks.len(),
            run.name,
            run.reward,
            if run.motion_ticks > 0 {
                run.board_motion / run.motion_ticks as f32
            } else {
                0.0
            },
        );
    }
}

#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn the_360_flip_family_ladders_by_rotation() {
    // ENDLESS from the **eighth**, which is one rung earlier than the cycle ladders turn over --
    // `degree_family!` in `scoring::extension` is the authority and it stops counting at the
    // seventh, because past 2520 degrees the number stops meaning anything on a trick that is one
    // rotation in retail. This read `>= 9` and disagreed with the table.
    let expected = |rung: usize, word: &str, endless: &str| -> String {
        if rung >= 8 {
            format!("#{endless}")
        } else {
            format!("#{} {word}", rung * 360)
        }
    };
    for (trick, word, endless) in [
        ("360Flip", "FLIP", "ENDLESS 360 FLIP"),
        ("Laserflip", "LASERFLIP", "ENDLESS LASERFLIP"),
    ] {
        let stock = replay(Input::Held { trick, boost: 14.0 }, 600);
        assert_eq!(
            stock.tricks.len(),
            1,
            "retail authors exactly one rung for {trick}"
        );

        // **Capped, like the kickflip's test is.** Uncapped at this boost the ladder takes seven
        // rungs, starts an eighth it has no air to finish, and wipes out banking nothing -- so the
        // reward assertion below measured how much air the flat course happens to give rather than
        // the naming it is here to check. The cap is the mod's own setting, so this exercises the
        // same switch a player has. Two rungs past the authored one is a 1080.
        let run = replay(
            Input::EndlessCapped { trick, boost: 14.0, extra: 2 },
            600,
        );
        assert!(
            run.tricks.len() > 1,
            "{trick}: the ladder never left the authored rung"
        );
        assert_eq!(
            run.name,
            expected(run.tricks.len(), word, endless),
            "{trick} at {} rungs",
            run.tricks.len()
        );
        assert!(
            run.reward > stock.reward,
            "{trick}: {} did not beat the single {}",
            run.reward,
            stock.reward
        );
    }
}

/// Endless Tricks, the point of the whole extension: given the air to afford them, a held flick
/// keeps cycling past the authored quad.
///
/// The authored ladder cannot produce this. `Cyc3` has no fifth cycle state and its only exit is
/// an unconditional `WillExpire` to `End.Out.Out4`, so `<Trick>3`/`<Trick>4` appearing a second
/// time in publication order is the synthesised loop and nothing else. The same pop with the
/// feature off is replayed beside it, so any difference is the feature rather than the boost.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn endless_flips_cycle_past_the_authored_quad() {
    // Measured window on the flat course: 11 m/s buys a fifth flip, 16 an eighth, and past
    // ~18 the skater leaves the course and never lands. 14 sits well inside it.
    let boost = 14.0;
    let stock = replay(Input::Held { trick: "Kickflip", boost }, 600);
    let endless = replay(
        Input::EndlessCapped {
            trick: "Kickflip",
            boost,
            extra: 3,
        },
        600,
    );
    assert_eq!(
        stock.tricks,
        ["Kickflip", "Kickflip2", "Kickflip3", "Kickflip4"],
        "the retail ladder stopped somewhere other than the authored quad"
    );
    assert_eq!(
        endless.tricks,
        ["Kickflip", "Kickflip2", "Kickflip3", "Kickflip4", "Kickflip5", "Kickflip6", "Kickflip7"],
        "the loop did not walk the extension rungs"
    );
    // Plain words rather than an `ID_TRICK_*` key: `apt_text::localize` echoes tokens it cannot
    // resolve, so an unknown key would reach the HUD as raw text.
    assert_eq!(endless.name, "#SEPTUPLE KICKFLIP");
    // Three rungs past the quad at the authored +50 a rung -- and the ladder still *converts*,
    // so the gain is rung 7 minus rung 4, not the sum of every rung beneath it.
    //
    // The window is a few points rather than one because the reward is not only the rungs: the
    // air metrics are measured off the flight, and the extra rungs replay the authored `_CYC2` /
    // `_CYC3` pair, whose lengths differ, so the trick ends a fraction of a second differently.
    // Measured at 151.52 against the three rungs' 150. It stays far below 50, so a rung that
    // failed to score still fails this.
    assert!(
        (endless.reward - stock.reward - 150.).abs() < 5.,
        "expected three +50 rungs over the quad's {}, got {}",
        stock.reward,
        endless.reward
    );
    // A rung the skater cannot land out of is worse than no rung at all, so the air budget has
    // to leave room for the out clip. A wipeout banks nothing and shows up here as a zero.
    assert!(
        endless.reward > 0.,
        "the endless run did not land cleanly: animations={:?}",
        endless.animations
    );
}

enum Input<'a> {
    /// Hold the ground scoop's final coordinate through the whole air.
    Held { trick: &'a str, boost: f32 },
    /// Flick and release, so the ladder ends at the single flip.
    Single { trick: &'a str, boost: f32 },
    /// Ollie, then run a `skater_air.pat` scoop once airborne.
    Late { gesture: &'a str, trick: &'a str },
    /// Endless Tricks: held exactly as `Held`, with the non-retail loop switched on. Everything
    /// else about the run is identical, so a difference in the outcome is the feature and
    /// nothing else.
    Endless { trick: &'a str, boost: f32 },
    /// The same, capped to a fixed number of extra rungs. Pinning the rung with the mod's own cap
    /// makes a naming test deterministic; anchoring it to a boost only measures how much air the
    /// test course happens to give before something else ends the run.
    EndlessCapped {
        trick: &'a str,
        boost: f32,
        extra: u32,
    },
}

struct Outcome {
    air_ticks: u32,
    clip_begins: u32,
    /// PROBE: mean per-tick movement of the animated body bones while airborne in a flip clip.
    body_motion: f32,
    /// PROBE: the same for the board bones, as the control -- the board must keep turning.
    board_motion: f32,
    motion_ticks: u32,
    /// Worst per-tick skeleton pose jump, as (tick, metres, clip).
    jolts: Vec<(u32, f32, String)>,
    /// PROBE: the same jump with the skeleton part index that produced it, as (tick, metres, part).
    jolt_parts: Vec<(u32, f32, usize)>,
    /// PROBE: the tick each rung was first published, so a rung can be shown to cost a whole clip.
    rung_ticks: Vec<(u32, String)>,
    /// PROBE: per-tick board movement with the clip that produced it, to separate a faster deck from
    /// faster rungs.
    board_per_clip: Vec<(String, f32, f32)>,
    animations: Vec<String>,
    tricks: Vec<String>,
    reward: f32,
    name: String,
}

fn replay(input: Input<'_>, ticks: u32) -> Outcome {
    let root = std::path::PathBuf::from(
        std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT"),
    );
    let assets = skate_data::GameAssets::load(&root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(&root, &assets).unwrap();
    let mut physics = GamePhysics::load_with_terrain(&root, ground::Terrain::Course).unwrap();
    let mut skater = SkaterRuntime::load(&root, &graphs, &physics, "normal").unwrap();
    // Endless Tricks arrives the way a mod delivers it, through the trainer table, so the test
    // exercises the same switch the game does. Off for every other run here, which is what makes
    // those runs proof that the compiled-in loop transitions are inert.
    physics.trainer.endless_flips = matches!(
        input,
        Input::Endless { .. } | Input::EndlessCapped { .. }
    );
    if let Input::EndlessCapped { extra, .. } = input {
        physics.trainer.endless_flip_max = extra;
    }
    let mut camera = crate::camera::CameraRuntime::load(&root).unwrap();
    let mut controller = crate::input::ControllerInput::default();
    let mut controls = PlayerControls::load(&root).unwrap();
    // The production `sample` system copies this from the trainer each tick; the harness
    // drives `publish_gestures` directly, so it sets it here.
    controls.endless_families = physics.trainer.endless_flips;

    let hold = !matches!(input, Input::Single { .. });
    // The authored scoops, straight out of the PATs the recognizer itself loads.
    let (ground_scoop, air_scoop, boost, watched) = match input {
        Input::Held { trick, boost }
        | Input::Single { trick, boost }
        | Input::Endless { trick, boost }
        | Input::EndlessCapped { trick, boost, .. } => (
            authored("skater.pat", &root, trick),
            Vec::new(),
            boost,
            ["", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"]
                .iter()
                .map(|count| format!("{trick}{count}"))
                .collect::<Vec<_>>(),
        ),
        Input::Late { gesture, trick } => (
            authored("skater.pat", &root, "Ollie"),
            authored("skater_air.pat", &root, gesture),
            9.0,
            vec![trick.to_owned()],
        ),
    };
    let watched: Vec<(String, AttributeName)> = watched
        .into_iter()
        .map(|name| {
            let encoded = encode(name.as_bytes());
            (name, encoded)
        })
        .collect();
    // GameInputManager 82696030 negates the mapped Y before matching.
    let sample = |p: [f32; 2]| [(p[0] * 32767.) as i16, (-p[1] * 32767.) as i16];

    let mut previous_pose: Option<Vec<[f32; 3]>> = None;
    let mut previous_animated: Option<Vec<[f32; 4]>> = None;
    let mut outcome = Outcome {
        air_ticks: 0,
        clip_begins: 0,
        body_motion: 0.,
        board_motion: 0.,
        motion_ticks: 0,
        jolts: Vec::new(),
        jolt_parts: Vec::new(),
        rung_ticks: Vec::new(),
        board_per_clip: Vec::new(),
        animations: Vec::new(),
        tricks: Vec::new(),
        reward: 0.,
        name: String::new(),
    };
    let mut air_ticks = 0u32;
    for tick in 0..ticks {
        if tick == 60 {
            // A rolling approach, without a steering input that would also spin.
            for body in physics.board.bodies_mut() {
                body.rates.linear_velocity = Vector3::new(0., 0., 8.);
            }
            for body in skater.skeleton.bodies_mut() {
                body.rates.linear_velocity = Vector3::new(0., 0., 8.);
            }
        }
        if tick == 123 {
            for body in physics.board.bodies_mut() {
                body.rates.linear_velocity.y += boost;
            }
            for body in skater.skeleton.bodies_mut() {
                body.rates.linear_velocity.y += boost;
            }
        }
        let right = if air_scoop.is_empty() {
            // Crouch on the first coordinate, then hold the flick: holding the
            // final coordinate is what keeps `<Trick>Hold` published.
            if ground_scoop.len() > 2 {
                // The 360 flip and laserflip gestures are four-point arcs -- out to one side, up
                // over the top, then down across. The recognizer walks every authored coordinate,
                // so skipping the middle two never completes the pattern and the trick silently
                // never fires. Two-point scoops keep the original timing below, untouched, so the
                // kickflip and heelflip measurements stay comparable.
                let last = ground_scoop.len() - 1;
                let step = 4;
                if tick < 100 {
                    [0; 2]
                } else {
                    let index = ((tick as usize - 100) / step).min(last);
                    if index < last || hold || tick < (100 + step * last + 6) as u32 {
                        sample(ground_scoop[index])
                    } else {
                        [0; 2]
                    }
                }
            } else if (100..112).contains(&tick) {
                sample(ground_scoop[0])
            } else if tick >= 112 && (hold || tick < 118) {
                sample(ground_scoop[ground_scoop.len() - 1])
            } else {
                [0; 2]
            }
        } else if (100..112).contains(&tick) {
            sample(ground_scoop[0])
        } else if (112..118).contains(&tick) {
            sample(ground_scoop[ground_scoop.len() - 1])
        } else if air_ticks >= 10 {
            // One coordinate every four ticks, comfortably inside the authored
            // culling window, and then release so the trick can end.
            let step = (air_ticks as usize - 10) / 4;
            air_scoop.get(step).copied().map(sample).unwrap_or([0; 2])
        } else {
            [0; 2]
        };
        controller.sample_raw_for_test(skate_core::input::xbox::XboxState {
            buttons: 0,
            triggers: [0; 2],
            left: [0; 2],
            right,
        });
        let mut actions = controller.player_actions();
        controls.update(
            &mut actions,
            physics.settings.step.simulation.time_step,
            physics.settings.input_magnitude_threshold,
            skater.player_input.physical.scoring.capabilities_204,
        );
        controls.publish_gestures(
            physics.animation_profile.physics_mode,
            skater.player_input.physical.state.state_16,
        );
        frame::advance(
            &mut physics,
            &mut skater,
            &mut controls,
            &graphs,
            &mut actions,
            true,
            &mut camera,
        )
        .unwrap();
        if skater.player_state.current().category() == 200 {
            air_ticks += 1;
            outcome.air_ticks += 1;
        }

        // PROBE: the animated pose itself, split the way the rig is -- body bones against board
        // bones. A working hold shows near-zero body movement while the board keeps turning.
        {
            let clip = skater
                .animation
                .motion
                .animation
                .current_name
                .clone()
                .unwrap_or_default();
            // Any air clip or cycle clip. This used to be `contains("FLIP_A")`, which silently
            // measured nothing for the pop shuvit families -- their clips are `360POPSHUVIT_HIGH_A`
            // and carry no "FLIP_A" -- and reported their board motion as 0.0000, which reads exactly
            // like a parked deck.
            if clip.ends_with("_A") || clip.contains("_CYC") {
                let pose = &skater.animation.pose;
                let bones = skater.animation.evaluator.body_bones();
                // Bone rotations carry the motion in a skeletal rig; translations are lengths.
                let sample: Vec<[f32; 4]> = pose.iter().map(|p| p.rotation).collect();
                if let Some(prev) = &previous_animated {
                    let delta = |i: usize| -> f32 {
                        let (a, b) = (&sample[i], &prev[i]);
                        (0..4).map(|k| (a[k] - b[k]).abs()).sum::<f32>()
                    };
                    let body: f32 = bones.iter().map(|&b| delta(b)).sum();
                    let board: f32 = (0..sample.len())
                        .filter(|i| !bones.contains(i))
                        .map(delta)
                        .sum();
                    outcome.body_motion += body;
                    outcome.board_motion += board;
                    // The length the engine's own clock is using, not the clip's nominal length. A
                    // rung that is cut short either has a shorter clock or fires before its end, and
                    // only this number tells those apart.
                    let length = skater.animation.motion.animation.current_length().unwrap_or(-1.0);
                    outcome.board_per_clip.push((clip.clone(), board, length));
                    outcome.motion_ticks += 1;
                }
                previous_animated = Some(sample);
            }
        }

        // PROBE: every skeleton part relative to the root, so bulk travel cancels and only
        // the pose change remains. A blend discontinuity shows up as a one-tick spike.
        {
            let bodies = skater.skeleton.bodies();
            let root = bodies[0].rates.position;
            let pose: Vec<[f32; 3]> = bodies
                .iter()
                .map(|b| {
                    let p = b.rates.position;
                    [p.x - root.x, p.y - root.y, p.z - root.z]
                })
                .collect();
            if let Some(prev) = &previous_pose {
                // Which part, not just how much: the board subtree and the four reparented helpers
                // sit above bone 25, so the index says whether a spike is the deck or the body. That
                // is what distinguishes a synthesised-spin fault from the hold pose arriving.
                let (worst, part) = pose
                    .iter()
                    .zip(prev)
                    .enumerate()
                    .map(|(i, (a, b))| {
                        let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
                        ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(), i)
                    })
                    .fold((0f32, 0usize), |acc, x| if x.0 > acc.0 { x } else { acc });
                outcome.jolt_parts.push((tick, worst, part));
                let clip = skater
                    .animation
                    .motion
                    .animation
                    .current_name
                    .clone()
                    .unwrap_or_default();
                outcome.jolts.push((tick, worst, clip));
            }
            previous_pose = Some(pose);
        }

        let motion = &skater.animation.motion;
        if motion
            .animation
            .current_name
            .as_deref()
            .is_some_and(|n| n.contains("_CYC") || n.contains("FLIP_A"))
            && motion.animation.property().crossed_end
        {
            outcome.clip_begins += 1;
        }
        if let Some(name) = motion.animation.current_name.as_deref() {
            if outcome.animations.last().map(String::as_str) != Some(name) {
                outcome.animations.push(name.to_owned());
            }
        }
        if let Some(published) = motion.score_packet.trick_names.first {
            if let Some((name, _)) = watched.iter().find(|(_, e)| *e == published) {
                if outcome.tricks.last() != Some(name) {
                    outcome.tricks.push(name.clone());
                    outcome.rung_ticks.push((tick, name.clone()));
                }
            }
        }
        outcome.reward = skater.scoring.session.holder.snapshot.last_reward;
        let published = skater.scoring.hud_input().trick_name;
        if !published.is_empty() && published != "#ID_TRICK_AIR" {
            outcome.name = published;
        }
    }
    eprintln!(
        "{:?}: tricks={:?} reward={} name={:?}\n  animations={:?}",
        watched.iter().map(|(n, _)| n).collect::<Vec<_>>(),
        outcome.tricks,
        outcome.reward,
        outcome.name,
        outcome.animations
    );
    outcome
}

fn authored(file: &str, root: &std::path::Path, name: &str) -> Vec<[f32; 2]> {
    skate_data::gesture_patterns::load(&root.join("private/stock/data/joystick").join(file))
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("authored {name} pattern in {file}"))
        .points
}
