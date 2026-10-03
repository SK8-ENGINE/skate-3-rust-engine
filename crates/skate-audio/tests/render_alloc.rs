//! The audio thread's render must not allocate in steady state (optimisation pass 2026-10-03,
//! doc 11 "Optimisation pass"): an allocation in the device callback can wait on the process heap
//! behind the game thread. A counting allocator wraps the system one; only this thread's
//! allocations between the markers count. The scene: AEMS voices (mono and stereo, filters on,
//! azimuths and cutoffs moving every evaluator tick) routed into the eEQChain buses and the
//! FlangeSub returns, the env network on a preset, and a bound grain truck with its full chain.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use skate_audio::bus::env::{DEFAULT_PRESET, Preset};
use skate_audio::bus::flange::FlangePreset;
use skate_audio::eval::{OpenRequest, VoiceHost};
use skate_audio::formats::SampleHeader;
use skate_audio::grain::player::{GrainParams, GrainSource, Record};
use skate_audio::mixer::Pcm;
use skate_audio::runtime::Runtime;

struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    /// This thread's counted allocations (each test reads its own).
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

fn count_one() {
    let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
}

fn allocations() -> u64 {
    ALLOCATIONS.with(Cell::get)
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            count_one();
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            count_one();
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn reverb01() -> Preset {
    Preset([
        4000.0, 600.0, 1.0, 0.08, 0.0, 1.5, 70.0, 1.0, 0.7, 4000.0, 500.0, 1.0, 1.0, 0.25, 1.0, 1.0, 3.0, 0.1, 0.34, 1.0, 1161.0, 0.5,
        0.2, 0.6, 1.0, 1.0, 270.0, 5000.0, 0.5, 0.45, 500.0, 0.75, 0.3, 0.3, 1.0, 1.0, 90.0, 5000.0, 0.51, 0.5, 500.0, 0.75, 0.28, 0.25,
    ])
}

fn scene() -> (Runtime, Vec<u32>) {
    let mut rt = Runtime::new();
    let frames = 44_100 * 4;
    let wave: Vec<f32> = (0..frames).map(|i| (i as f32 * 0.03).sin() * 0.3).collect();
    let mono = Arc::new(Pcm { rate: 44_100, channels: vec![wave.clone()] });
    let stereo = Arc::new(Pcm { rate: 44_100, channels: vec![wave.clone(), wave.clone()] });
    let header = |channels| SampleHeader { codec: 3, channels, rate: 44_100, frames: frames as u32, loop_start: Some(0) };
    rt.mixer.add_bank(0, vec![Some(header(1)), Some(header(2))], vec![Some(mono), Some(stereo)]);
    rt.mixer.buses.env.presets.insert(DEFAULT_PRESET, reverb01());
    rt.mixer.buses.env.request(DEFAULT_PRESET);
    rt.mixer.buses.flange.set_presets(
        FlangePreset([20.0, 0.3, 0.2, 0.1, 1500.0, 0.002, 0.5, 0.9, 0.03]),
        FlangePreset([1.7, 0.3, 0.0, 1.0, 250.0, 0.0005, 0.25, 0.6, 0.11]),
    );
    rt.mixer.buses.flange.frame([32692, 2313, 32692, 2313]);
    rt.mixer.buses.eq.clear(Some([5000.0, 1.5, 2.0, 2000.0, 0.8, 3.0]));
    let routes: Vec<[(u8, i32); 4]> = (0..8).map(|b| [(9u8, b), (10, 4096), (11, 1), (12, 1638)]).collect();
    let ids = (0..24)
        .filter_map(|k| {
            rt.mixer.open(&OpenRequest { bank: 0, slot: (k % 2) as u16, level: 100, azimuth: [224, 32, 0, 0, 0, 0], stream_offset: u32::MAX, inputs: &routes[k % 8] })
        })
        .collect();
    let source = Arc::new(GrainSource { name: "t".into(), duration: 4.0, pcm: Arc::new(Pcm { rate: 44_100, channels: vec![wave] }) });
    let a = GrainParams { attack: 0.1, sustain: 0.2, release: 0.1, window: 1.6, drift: 0.05 };
    let b = GrainParams { attack: 0.2, sustain: 0.1, release: 0.2, window: 1.5, drift: 0.05 };
    rt.grains.bind_truck(0, source, [a, b], [Record { gain: 0.5, pitch: 1.0, position: 0.3 }, Record { gain: 0.25, pitch: 1.0, position: 0.2 }]);
    (rt, ids)
}

#[test]
fn steady_state_render_does_not_allocate() {
    let (mut rt, ids) = scene();
    let before = allocations();
    let mut measured = 0;
    for block in 0..400usize {
        if block % 6 == 0 {
            for (k, &v) in ids.iter().enumerate() {
                rt.mixer.set_azimuth(v, ((block * 37 + k * 911) % 65536) as i32);
                rt.mixer.set(v, 6, 2000 + ((block + k * 13) % 20000) as i32);
                rt.mixer.set(v, 7, 77);
                rt.mixer.set(v, 2, 20000);
                rt.mixer.set(v, 5, 3000);
                rt.mixer.set(v, 0, 4096 + ((block + k) % 400) as i32);
            }
        }
        // Warm-up: the first blocks size the delay lines, reverb combs and the voices' caches.
        let count = block >= 100;
        COUNTING.with(|c| c.set(count));
        let _ = rt.render_block();
        COUNTING.with(|c| c.set(false));
        measured += usize::from(count);
    }
    let n = allocations() - before;
    eprintln!("{n} allocations in {measured} steady-state blocks ({} voices)", rt.mixer.voice_count());
    assert_eq!(n, 0, "render_block allocated {n} times in {measured} blocks");
}

/// The game thread's per-frame calls under the runtime lock must not allocate either (PR #32
/// review, 2026-10-03): with an installed (hand-built) AEMS bank and a posted program playing,
/// `redeliver` every console frame, `release` and the evaluator walks inside `render_block` run
/// without an allocation once warm (the walk reuses its order snapshot; `redeliver` / `release`
/// read their client lists in place; a destroy reads the module's object lists in place).
#[test]
fn installed_bank_redeliver_release_and_walks_do_not_allocate() {
    use skate_audio::eval::synthetic::{Ex, bank, player_module, project};
    let (mut rt, _) = scene();
    rt.eval.install_project(&project());
    let pcm = |frames: usize| Some(Arc::new(Pcm { rate: 48_000, channels: vec![(0..frames).map(|i| (i as f32 * 0.01).sin() * 0.2).collect()] }));
    let id = rt.load_bank(bank(&[player_module(4)], &[Ex { module: 0, kind: 1, name_id: 1, name: "c_test", at: None }], &[(48_000, true), (24_000, true)]), vec![pcm(48_000), pcm(24_000)]);
    let class = rt.eval.class_id("c_test").expect("the bank answers c_test");
    let node = rt.post(class, &[1, 4096, 1]);
    let doomed = rt.post(class, &[1, 4096, 0]);
    // A release during the warm-up too: the first destroy sizes the evaluator's free list.
    let warm = rt.post(class, &[1, 4096, 0]);
    let before = allocations();
    let mut measured = 0;
    for block in 0..600usize {
        // Warm-up: the first walks create the instances and open their voices (allocating).
        let count = block >= 120;
        COUNTING.with(|c| c.set(count));
        if block % 6 == 0 {
            rt.redeliver(node, &[1, 4096 + (block % 600) as i32, 1]);
        }
        if block == 30 {
            rt.release(warm);
        }
        if block == 300 {
            rt.release(doomed);
        }
        let _ = rt.render_block();
        COUNTING.with(|c| c.set(false));
        measured += usize::from(count);
    }
    assert!(rt.eval.bank(id).is_some(), "the bank stayed installed");
    let n = allocations() - before;
    eprintln!("{n} allocations in {measured} blocks with an installed bank ({} voices)", rt.mixer.voice_count());
    assert!(rt.mixer.voice_count() > 0, "the program plays");
    assert_eq!(n, 0, "redeliver / release / walks allocated {n} times in {measured} blocks");
}
