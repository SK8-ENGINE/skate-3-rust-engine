//! End-to-end render of the native player audio (MixMap inputs → components → AEMS voices →
//! granular bed → 6-channel bus) for the scripted per-frame situations of
//! `tools/audio-e2e/scenarios.py`, headless. The same scripts can drive another renderer (e.g. the
//! PoC's oracle probe); `tools/audio-e2e/compare.py` compares two renders.
//!
//!   set E2E_DIR=...\.local\audio-re\e2e   (optional E2E_ONLY=roll20,grind_metal)
//!   cargo test -p skate-game --release --bin skate3rust -- --ignored e2e_render --nocapture
//!
//! Writes `<name>.ours.f32` (raw f32, 6 channels in the PoC's order L, R, C, LFE, Ls, Rs) and
//! `<name>.ours.voices.tsv` (per frame: bank, slot, gain, pitch of every sounding voice; grain
//! voices as `grain`).
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use skate_audio::formats::{Bank, Project};
use skate_audio::mixmap::{MixMap, keys};
use skate_audio::runtime::Runtime;

use super::player_audio::{BANKS, PlayerAudio};

use super::state_replay::{Script, read};

fn globals(m: &mut MixMap) {
    for id in 1..=4 {
        m.set_input(keys::MASTER, id, 32767);
    }
    for id in [1, 2, 5] {
        m.set_input(keys::MUSIC, id, 32767);
    }
    m.set_input(keys::REVERB, 5, 32767);
}

#[test]
#[ignore = "headless render for the PoC comparison; needs E2E_DIR and the install"]
fn e2e_render() {
    let Some(dir) = std::env::var_os("E2E_DIR").map(std::path::PathBuf::from) else { return eprintln!("E2E_DIR not set") };
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let library = super::Library::load(root).expect("install");
    let mxb = library.read(library.aems().mixmap.as_ref().expect("MixMap")).unwrap();
    let only = std::env::var("E2E_ONLY").ok();
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "tsv") && !p.file_stem().unwrap().to_string_lossy().contains('.')).collect();
    paths.sort();
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if only.as_ref().is_some_and(|o| !o.split(',').any(|x| x == name)) {
            continue;
        }
        let rows = read(&path);
        let mut rt = Runtime::new();
        for file in &library.aems().projects {
            rt.install_project(&Project::parse(file, &library.read(file).unwrap()).unwrap());
        }
        let mut names = HashMap::new();
        for stem in std::iter::once("emitter_utility").chain(BANKS.iter().copied()) {
            let file = &library.aems().banks[stem];
            let bank = Bank::parse(stem, library.read(file).unwrap()).unwrap();
            let id = rt.load_bank(bank, library.bank_pcm(stem));
            names.insert(id, stem);
        }
        let utility = rt.eval.class_id("c_emitter_utility").unwrap();
        rt.post(utility, &[]);
        let off = |var: &str| std::env::var(var).is_ok_and(|v| v == "0");
        // Retail's second boot utility (the Seams program's sample shuffles); E2E_SEAM_UTILITY=0:
        // without it, as before Listening test 8.
        if !off("E2E_SEAM_UTILITY") {
            use skate_audio::player::seams::{UTILITY, UTILITY_BANK};
            if let Some(file) = library.aems().banks.get(UTILITY_BANK) {
                let id = rt.load_bank(Bank::parse(UTILITY_BANK, library.read(file).unwrap()).unwrap(), Vec::new());
                names.insert(id, UTILITY_BANK);
                rt.post(rt.eval.class_id(UTILITY).unwrap(), &[]);
            }
        }
        // The optional components (rolling layers, rattle, board slide, tricks, treatment) run when
        // their first bank is in the install, as in the game; E2E_<NAME>=0 turns one off.
        let off = |var: &str| std::env::var(var).is_ok_and(|v| v == "0");
        let mut optional = [false; 5];
        for (k, (banks, var)) in super::player_audio::OPTIONAL_BANKS.iter().zip(["E2E_ROLLING", "E2E_RATTLE", "E2E_SLIDE", "E2E_TRICKS", "E2E_TREATMENT"]).enumerate() {
            if off(var) {
                continue;
            }
            for (i, stem) in banks.iter().enumerate() {
                let Some(file) = library.aems().banks.get(*stem) else { continue };
                let bank = Bank::parse(stem, library.read(file).unwrap()).unwrap();
                let id = rt.load_bank(bank, library.bank_pcm(stem));
                names.insert(id, stem);
                optional[k] |= i == 0;
            }
        }
        if optional[3] {
            if let Some(id) = rt.eval.class_id(skate_audio::player::tricks::FOLEY_UTILITY) {
                rt.post(id, &[]);
            }
        }
        // E2E_CHAIN=0: the bed's chain as before the full graph-1/2/3 port.
        rt.grains.chain_extras = !off("E2E_CHAIN");
        let mut contacts = true;
        for (i, stem) in super::player_audio::SPLICE_BANKS.iter().enumerate() {
            match library.splice_bank(stem) {
                Some((bank, pcm)) => {
                    let r = &mut rt;
                    r.splice.load_bank(stem, bank, pcm, &mut r.mixer);
                }
                None => contacts &= i > 0,
            }
        }
        let streams: Vec<_> = super::player_audio::WHEEL_STREAMS.iter().map(|n| library.wheels_pcm(n)).collect();
        let wheels = streams.iter().all(Option::is_some);
        rt.load_streams(streams);
        // The buses (E2E_BUSES=0: dry, as before the bus port): the reverb network at the default
        // preset (no map → reverb01) and the eEQChain buses.
        if std::env::var("E2E_BUSES").map_or(true, |v| v != "0") {
            let (presets, eq) = library.bus_tuning();
            rt.mixer.buses.env.presets = presets;
            rt.mixer.buses.eq.set_records(&eq);
            // E2E_PRESET=<16 hex digits>: a region's preset key instead (e.g. University's
            // 407AFA1D6C7CEAD8, DownTown's 68A9E6020DF2076D).
            let preset = std::env::var("E2E_PRESET").ok().and_then(|k| u64::from_str_radix(&k, 16).ok());
            rt.mixer.buses.env.request(preset.unwrap_or(skate_audio::bus::env::DEFAULT_PRESET));
            // The FlangeSub returns (E2E_FLANGE=0: off).
            if let Some([a, b]) = library.flange_presets().filter(|_| !off("E2E_FLANGE")) {
                rt.mixer.buses.flange.set_presets(a, b);
            }
        }
        // The FootStep SubMix graphs.
        rt.mixer.buses.submix.enabled = true;
        let shared = Arc::new(Mutex::new(rt));
        let mut m = MixMap::from_bytes(&mxb).unwrap();
        let mut p = PlayerAudio::new(library.player_tuning(), true);
        p.contacts_on = contacts && std::env::var("E2E_CONTACTS").map_or(true, |v| v != "0");
        p.contact_tuning = library.contacts_tuning();
        p.footsteps_on = p.contacts_on && !off("E2E_FOOTSTEPS");
        // E2E_BODY_LOG=1: every body-poster message (`<name>.ours.bodymsg.tsv`).
        let body_log = std::env::var("E2E_BODY_LOG").is_ok_and(|v| v == "1");
        p.set_body_log(body_log);
        p.set_footstep_materials(library.footstep_materials());
        p.wheels_on = wheels && std::env::var("E2E_WHEELS").map_or(true, |v| v != "0");
        (p.rolling_on, p.rattle_on, p.slide_on, p.tricks_on, p.treatment_on) = (optional[0], optional[1], optional[2], optional[3], optional[4]);
        let mut bed = super::grain_bed::Bed::new(&library).expect("grain bed data");
        // E2E_SLEW_LOG=1: the bed's turn input, |COM v|, special, I and brake per call
        // (`<name>.ours.slew.tsv`).
        let mut slew_log = std::env::var("E2E_SLEW_LOG").is_ok_and(|v| v == "1").then(|| {
            let mut w = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.slew.tsv"))).unwrap());
            writeln!(w, "frame	eval	turn_in	com_speed	turn_I	turn_signed	braking	brake").unwrap();
            w
        });
        let seams = skate_audio::player::tuning::PlayerTuning { seam_wobbles: p.tuning.seam_wobbles.clone(), ..Default::default() };
        let mut script = Script::default();
        let mut out = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.f32"))).unwrap());
        let mut voices = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.voices.tsv"))).unwrap());
        writeln!(voices, "frame\tbank\tslot\tgain\tpitch").unwrap();
        // The body poster's messages: the row where its count changed, the count and the digest.
        let mut body = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.body.tsv"))).unwrap());
        writeln!(body, "frame\tposts\tdigest").unwrap();
        let mut body_seen = p.body_trace();
        let mut bodymsg = body_log.then(|| {
            let mut w = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.bodymsg.tsv"))).unwrap());
            writeln!(w, "frame\trow\tregion\timpact\tcom216\tmat_a\tmat_b\ttier_a\ttier_b\tlevel_a\tlevel_b").unwrap();
            w
        });
        // The deck poster's messages, the same way.
        let mut deck = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.deck.tsv"))).unwrap());
        writeln!(deck, "frame\tposts\tdigest").unwrap();
        let mut deck_seen = p.deck_trace();
        let (mut owed, blocks_per_frame) = (0.0f64, 48_000.0 / 256.0 / 60.0);
        let order: Vec<usize> = std::iter::repeat(0).take(60).chain(0..rows.len()).collect();
        // E2E_TIMING=1: per-frame cost of the game-thread side and per-block cost of the render.
        let timing = std::env::var("E2E_TIMING").is_ok_and(|v| v == "1");
        let (mut game_us, mut block_us): (Vec<(f64, usize, [f64; 5])>, Vec<f64>) = (Vec::new(), Vec::new());
        // E2E_SUBSTEPS=N: N audio-manager calls per 60 Hz physics row (the recomp calls it per
        // rendered frame, ~5 per physics step: the state, wheel positions included, changes only on
        // the first). Diagnostic for cadence-dependent components (Class_Seams' one-call w7 pulse);
        // 1 (default) = the game's 60 Hz host.
        let substeps: usize = std::env::var("E2E_SUBSTEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
        let sub_dt = 1.0 / 60.0 / substeps as f32;
        // E2E_CALLS=N: the game's host at N rendered frames per 60 Hz tick (353 fps ≈ 6): the full
        // tick on the first call; Class_Seams on its console cadence (`PlayerAudio::seam_frame`) on
        // every call, at the wheel positions interpolated from the previous row like the rendered
        // board; the blocks spread evenly. 1 (default) = one call per row (the game's host at 60 fps).
        let calls: usize = std::env::var("E2E_CALLS").ok().and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
        let per_row = substeps * calls;
        // E2E_FPS=f (overrides E2E_CALLS): the game's host at a rendered frame rate f, as
        // `native::mixmap_frame` runs it: f/60 calls per physics row on average (the first call of a
        // row ticks; every call runs `seam_frame`); below 60 fps a call ticks the MixMap once per
        // elapsed 60 Hz step and rows without a call only render.
        let fps: Option<f64> = std::env::var("E2E_FPS").ok().and_then(|v| v.parse().ok());
        // The MixMap's console cadence (`native::mixmap_frame`, `skate_audio::mixmap::cadence`): one
        // evaluation per two 60 Hz rows with dt 1/30, the Jitter stepped and the eEQChain cleared
        // there, the flag inputs held in between (2026-10-03: the only host; the per-row renders
        // before it ran the old 60 Hz evaluation, see doc 11 "A/B switches removed").
        // The game's host clock (`native::HostClock`): each row is a physics step published as
        // `skate_events::observe` publishes it (one-step pulses latched until a pass takes them);
        // a pass takes the steps since the last one (capped), as `native::mixmap_frame`.
        let mut clock = super::native::HostClock::default();
        let mut cues = super::skate_events::Cues::default();
        super::native::hold_flag_inputs(&mut m);
        let mut fps_acc = 0.0f64;
        for (frame, &i) in order.iter().enumerate() {
          let row_riding = script.riding(&rows[i]);
          cues.publish(row_riding);
          let (n_row, calls) = match fps {
              Some(f) => {
                  fps_acc += f / 60.0;
                  let n = fps_acc.floor() as usize;
                  fps_acc -= n as f64;
                  (n, n.max(1))
              }
              None => (per_row, calls),
          };
          let per_row = n_row.max(1);
          if n_row == 0 {
              owed += blocks_per_frame;
              let mut rt = shared.lock().unwrap();
              while owed >= 1.0 {
                  owed -= 1.0;
                  let bus = rt.render_block();
                  for k in 0..skate_audio::BLOCK {
                      for ch in [0, 2, 1, 5, 3, 4] {
                          out.write_all(&bus[ch][k].to_le_bytes()).unwrap();
                      }
                  }
              }
          }
          for call in 0..n_row {
            // The console seams cadence (Listening test 9): every call is a rendered frame of 1/f s.
            {
                p.seam_alpha = Some((call % calls) as f32 / calls as f32);
                let dt = fps.map_or(1.0 / (60.0 * calls as f64), |f| 1.0 / f) as f32;
                p.step_wheels(cues.riding.wheels_before, cues.riding.audio.wheel_position);
                p.seam_frame(&m, &cues.riding.audio, dt, &mut shared.lock().unwrap());
            }
            if call % calls != 0 {
                owed += blocks_per_frame / per_row as f64;
                let mut rt = shared.lock().unwrap();
                while owed >= 1.0 {
                    owed -= 1.0;
                    let bus = rt.render_block();
                    for k in 0..skate_audio::BLOCK {
                        for ch in [0, 2, 1, 5, 3, 4] {
                            out.write_all(&bus[ch][k].to_le_bytes()).unwrap();
                        }
                    }
                }
                continue;
            }
            let t0 = std::time::Instant::now();
            // E2E_SUBSTEPS: every ticking call of a row is a pass on the row's sample.
            if call > 0 {
                cues.publish(row_riding);
            }
            let pass = clock.pass(&mut cues, false).expect("a ticking call follows a published row");
            let riding = cues.riding;
            let mut s = riding.audio;
            if substeps > 1 {
                s.dt = sub_dt;
            }
            globals(&mut m);
            {
                let v = shared.lock().unwrap().mixer.buses.env.reverb_inputs();
                for (id, x) in v.into_iter().enumerate() {
                    m.set_input(keys::REVERB, id, x);
                }
            }
            // Below 60 fps one call ticks once per physics step since the last call (the game's
            // `HostClock`), and the listener's velocity and the bed's frame span those steps.
            let ticks = pass.ticks;
            let call_dt = sub_dt * ticks as f32;
            let l = p.listener([script.x - 3.0, 2.5, 0.0], [1.0, 0.0, 0.0], call_dt, &s);
            let mix_calls = pass.calls;
            p.jitter_steps = Some(mix_calls);
            // As `mixmap_frame`: sub_82491180 (half 1) before the inputs, with the last walk's values.
            if mix_calls > 0 {
                shared.lock().unwrap().mixer.buses.eq.clear(p.eq_jitter());
            }
            p.write_inputs(&mut m, &s, Some(&l));
            bed.write_inputs(&mut m, &s, !p.rolling_on);
            let speed_scale = bed.push_scale();
            let loose = PlayerAudio::loose_board(&s, &riding);
            let t1 = std::time::Instant::now();
            p.process(&mut m, &s, &mut shared.lock().unwrap(), speed_scale, loose);
            let t2 = std::time::Instant::now();
            if p.body_trace() != body_seen {
                body_seen = p.body_trace();
                writeln!(body, "{frame}\t{}\t{:016x}", body_seen.0, body_seen.1).unwrap();
            }
            if let Some(w) = bodymsg.as_mut() {
                for (region, impact, m) in p.take_body_log() {
                    writeln!(w, "{frame}\t{i}\t{region}\t{impact}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", s.com_speed_216, m.material[0], m.material[1], m.tier[0], m.tier[1], m.level[0], m.level[1]).unwrap();
                }
            }
            if p.deck_trace() != deck_seen {
                deck_seen = p.deck_trace();
                writeln!(deck, "{frame}\t{}\t{:016x}", deck_seen.0, deck_seen.1).unwrap();
            }
            for _ in 0..mix_calls {
                m.tick(skate_audio::mixmap::cadence::CONSOLE_DT);
            }
            let t3 = std::time::Instant::now();
            p.update(&m, &s, &mut shared.lock().unwrap(), speed_scale, loose);
            shared.lock().unwrap().mixer.buses.flange.frame(std::array::from_fn(|i| m.level(keys::REVERB, i)));
            shared.lock().unwrap().mixer.buses.env.scale_frame(m.level(keys::REVERB, 4));
            let t4 = std::time::Instant::now();
            let routed = p.rolling_on.then(|| (std::mem::take(&mut p.routed.grains), p.routed.primary));
            bed.slew_calls = Some(mix_calls);
            super::grain_bed::step(&mut bed, &library, &m, &shared, &riding, call_dt, &seams, routed);
            if let Some(w) = slew_log.as_mut() {
                let (turn, brake) = bed.slews();
                writeln!(w, "{frame}	{mix_calls}	{}	{}	{}	{turn}	{}	{brake}", s.turn, s.com_speed(), turn.abs(), u8::from(riding.braking)).unwrap();
            }
            let t5 = std::time::Instant::now();
            if timing && frame >= 60 {
                let us = |a: std::time::Instant, b: std::time::Instant| (b - a).as_secs_f64() * 1e6;
                game_us.push((us(t0, t5), frame - 60, [us(t0, t1), us(t1, t2), us(t2, t3), us(t3, t4), us(t4, t5)]));
            }
            owed += blocks_per_frame / per_row as f64;
            let mut rt = shared.lock().unwrap();
            while owed >= 1.0 {
                owed -= 1.0;
                let tb = std::time::Instant::now();
                let bus = rt.render_block();
                if timing && frame >= 60 {
                    block_us.push(tb.elapsed().as_secs_f64() * 1e6);
                }
                // Ours: L, C, R, Ls, Rs, LFE → the PoC's L, R, C, LFE, Ls, Rs.
                for k in 0..skate_audio::BLOCK {
                    for ch in [0, 2, 1, 5, 3, 4] {
                        out.write_all(&bus[ch][k].to_le_bytes()).unwrap();
                    }
                }
            }
            drop(rt);
          }
            let rt = shared.lock().unwrap();
            if frame >= 60 {
                for v in rt.mixer.snapshot() {
                    let bank = names.get(&v.bank).copied().unwrap_or("?");
                    let out = match v.output {
                        skate_audio::bus::Output::Master => 8,
                        skate_audio::bus::Output::Eq(i) => i,
                        // A FootStep SubMix graph (sends on into the env bus and SFX Master).
                        skate_audio::bus::Output::Submix(i) => 20 + i,
                    };
                    writeln!(voices, "{}\t{bank}\t{}\t{:.5}\t{:.4}\t{:.5}\t{out}", frame - 60, v.slot, v.gain, v.pitch, v.send).unwrap();
                }
                for (t, truck) in rt.grains.trucks.iter().enumerate() {
                    for (k, player) in truck.players.iter().enumerate() {
                        if player.voices() > 0 {
                            let r = player.record;
                            writeln!(voices, "{}\tgrain{t}{}\t{}\t{:.5}\t{:.4}\t{:.4}", frame - 60, ["A", "B"][k], player.starts, r.gain, r.pitch, r.position).unwrap();
                        }
                    }
                }
                for (bank, record, sample, gain, pitch) in rt.splice.voices() {
                    writeln!(voices, "{}	splice:{bank}:{record}	{sample}	{gain:.5}	{pitch:.4}", frame - 60).unwrap();
                }
                if rt.grains.rocket.voices() > 0 {
                    let r = rt.grains.rocket.record;
                    writeln!(voices, "{}\trocket\t{}\t{:.5}\t{:.4}\t{:.4}", frame - 60, rt.grains.rocket.starts, r.gain, r.pitch, r.position).unwrap();
                }
            }
        }
        let (hits, transitions) = p.seam_hits();
        eprintln!("{name}: {} frames (+60 settle); Class_Seams hits {hits} ({transitions} material changes)", rows.len());
        if timing && !game_us.is_empty() {
            // The raw times for the optimisation bench (`tools/audio-bench/`):
            // one line per measured game-thread call and per rendered block.
            let mut raw = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.timing.tsv"))).unwrap());
            writeln!(raw, "kind\tus").unwrap();
            for (total, _, _) in &game_us {
                writeln!(raw, "frame\t{total:.2}").unwrap();
            }
            for us in &block_us {
                writeln!(raw, "block\t{us:.2}").unwrap();
            }
            drop(raw);
            let pct = |v: &mut Vec<f64>, q: f64| {
                v.sort_by(f64::total_cmp);
                v[((v.len() - 1) as f64 * q) as usize]
            };
            let mut g: Vec<f64> = game_us.iter().map(|x| x.0).collect();
            eprintln!("  game thread per frame (us): p50 {:.0} p99 {:.0} max {:.0}", pct(&mut g, 0.5), pct(&mut g, 0.99), pct(&mut g, 1.0));
            let mut slow: Vec<(usize, f64)> = block_us.iter().copied().enumerate().collect();
            slow.sort_by(|a, b| b.1.total_cmp(&a.1));
            eprintln!("  slowest blocks (index, us): {:?}", &slow[..slow.len().min(4)]);
            eprintln!("  render per 256-frame block (us, budget 5333): p50 {:.0} p99 {:.0} max {:.0}", pct(&mut block_us, 0.5), pct(&mut block_us, 0.99), pct(&mut block_us, 1.0));
            game_us.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (total, f, parts) in game_us.iter().take(8) {
                eprintln!("  frame {f}: {total:.0} us = inputs {:.0} process {:.0} tick {:.0} update {:.0} bed {:.0}", parts[0], parts[1], parts[2], parts[3], parts[4]);
            }
        }
    }
}
