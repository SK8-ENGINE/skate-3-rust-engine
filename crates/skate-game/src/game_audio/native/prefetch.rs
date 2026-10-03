//! World emitter banks read and decoded ahead of need, off the game thread (doc 11, "Emitter
//! bank prefetch"). Without it `Native::ensure_bank` read the `.abk` and decoded every WAV on the
//! game thread when a bank's first emitter started: 2.6–5.4 ms warm, up to 36 ms from a cold disk.
//!
//! - `emitters::update` asks for the banks of the emitters the listener is within
//!   [`AHEAD`] metres of (bounding sphere), nearest first, and drops the ones it moved [`EVICT`]
//!   metres away from before they were used. A map's emitter banks decode to 160–250 MiB in the
//!   districts, so only a ring around the listener is held (~20–30 MiB in the users' sessions).
//! - One worker thread (started on the first request) runs [`BankSource::load`], the same function
//!   the game thread runs without a prefetch, so the bank and PCM are identical.
//! - Only the data moves: `Runtime::load_bank` still runs in `ensure_bank`, at the emitter's start,
//!   so the runtime (bank ids, evaluator, mixer, random draws) sees exactly what it saw before.
//! - [`Prefetch::take`] at the start: done → the data; being decoded → wait for it (at most one
//!   bank's decode, never longer than loading it here); queued, failed or unknown → None, and the
//!   caller loads it on the game thread as before. A start is never delayed or deferred.
use std::collections::HashMap;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

use super::super::library::{BankPcm, BankSource};
use skate_audio::formats::Bank;

/// Prefetch a bank when the listener is this close to the edge of one of its emitters (m).
pub(crate) const AHEAD: f32 = 60.0;
/// Drop a prefetched, unused bank when the listener is this far from all its emitters (m).
pub(crate) const EVICT: f32 = 90.0;

enum Stage {
    Queued,
    Running,
    Done(Bank, BankPcm),
    /// The load failed or panicked, or the slot was dropped / taken before the worker reached it.
    Skip,
}

struct Slot {
    stage: Mutex<Stage>,
    ready: Condvar,
}

enum Job {
    Load(Arc<Slot>, BankSource),
    /// A dropped bank's data, freed on the worker (megabytes of PCM).
    Free(Box<(Bank, BankPcm)>),
}

/// The requested banks (stem → slot) and the worker's queue.
#[derive(Default)]
pub(crate) struct Prefetch {
    jobs: Option<Sender<Job>>,
    slots: HashMap<String, Arc<Slot>>,
    /// [`Prefetch::clear`] calls so far: a requester that remembers what it asked for (the world
    /// sources) knows its requests are gone when this changes.
    clears: u64,
}

impl Prefetch {

    /// Queue the bank for the worker (no-op when it is queued already).
    pub(crate) fn request(&mut self, source: BankSource) {
        if self.slots.contains_key(source.stem()) {
            return;
        }
        let slot = Arc::new(Slot { stage: Mutex::new(Stage::Queued), ready: Condvar::new() });
        let jobs = self.jobs.get_or_insert_with(spawn);
        let stem = source.stem().to_owned();
        if jobs.send(Job::Load(slot.clone(), source)).is_ok() {
            self.slots.insert(stem, slot);
        } else {
            // The worker is gone: loads stay on the game thread.
            self.jobs = None;
        }
    }

    /// The prefetched bank, if the worker has it or is decoding it (then wait for it). None: not
    /// requested, not started yet (the worker skips it now) or failed; the caller loads it itself.
    pub(crate) fn take(&mut self, stem: &str) -> Option<(Bank, BankPcm)> {
        let slot = self.slots.remove(stem)?;
        let Ok(stage) = slot.stage.lock() else { return None };
        let Ok(mut stage) = slot.ready.wait_while(stage, |s| matches!(s, Stage::Running)) else { return None };
        match std::mem::replace(&mut *stage, Stage::Skip) {
            Stage::Done(bank, pcm) => Some((bank, pcm)),
            _ => None,
        }
    }

    /// Forget a requested bank (queued: the worker skips it; done: its memory is freed).
    pub(crate) fn drop_bank(&mut self, stem: &str) {
        if let Some(slot) = self.slots.remove(stem) {
            self.free(skip(&slot));
        }
    }

    /// Forget every requested bank (map change).
    pub(crate) fn clear(&mut self) {
        self.clears += 1;
        let slots: Vec<_> = self.slots.drain().map(|(_, slot)| slot).collect();
        for slot in slots {
            self.free(skip(&slot));
        }
    }

    /// Whether the bank was requested and not taken or dropped since.
    pub(crate) fn contains(&self, stem: &str) -> bool {
        self.slots.contains_key(stem)
    }

    /// How many times [`Prefetch::clear`] ran (see the field).
    pub(crate) fn clears(&self) -> u64 {
        self.clears
    }

    /// Hand a dropped bank's data to the worker to free (dropped here when there is no worker).
    fn free(&self, data: Option<(Bank, BankPcm)>) {
        if let (Some(data), Some(jobs)) = (data, &self.jobs) {
            let _ = jobs.send(Job::Free(Box::new(data)));
        }
    }

}

/// Queued → skipped; done → its data returned (to be freed). A running load finishes on the
/// worker and is dropped there with the slot.
fn skip(slot: &Slot) -> Option<(Bank, BankPcm)> {
    let mut stage = slot.stage.lock().ok()?;
    if matches!(*stage, Stage::Running) {
        return None;
    }
    match std::mem::replace(&mut *stage, Stage::Skip) {
        Stage::Done(bank, pcm) => Some((bank, pcm)),
        _ => None,
    }
}

fn spawn() -> Sender<Job> {
    let (tx, rx) = channel::<Job>();
    let worker = std::thread::Builder::new().name("audio-bank-prefetch".into()).spawn(move || {
        // Ends when the `Native` resource (the sender) is dropped.
        for job in rx {
            let (slot, source) = match job {
                Job::Load(slot, source) => (slot, source),
                Job::Free(data) => {
                    drop(data);
                    continue;
                }
            };
            {
                let Ok(mut stage) = slot.stage.lock() else { continue };
                if !matches!(*stage, Stage::Queued) {
                    continue;
                }
                *stage = Stage::Running;
            }
            // A failed or panicking load is left to the game thread, which then fails the same
            // way it always did.
            let result = std::panic::catch_unwind(|| source.load()).ok().and_then(Result::ok);
            let mut stage = match slot.stage.lock() {
                Ok(stage) => stage,
                Err(poisoned) => poisoned.into_inner(),
            };
            *stage = match result {
                Some((bank, pcm)) => Stage::Done(bank, pcm),
                None => Stage::Skip,
            };
            drop(stage);
            slot.ready.notify_all();
        }
    });
    if worker.is_err() {
        // No thread: the receiver is gone, so `request` falls back to game-thread loads.
        return channel::<Job>().0;
    }
    tx
}

#[cfg(test)]
impl Prefetch {
    /// The requested stems.
    pub(crate) fn stems(&self) -> impl Iterator<Item = &str> {
        self.slots.keys().map(String::as_str)
    }

    /// Wait until the worker has finished (or skipped) the bank.
    pub(crate) fn wait(&self, stem: &str) {
        if let Some(slot) = self.slots.get(stem) {
            let stage = slot.stage.lock().unwrap();
            drop(slot.ready.wait_while(stage, |s| matches!(s, Stage::Queued | Stage::Running)).unwrap());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::Library;
    use super::super::super::library::WAV_DECODES;
    use super::super::Native;
    use super::*;
    use std::time::Instant;

    fn decodes() -> u64 {
        WAV_DECODES.with(|n| n.get())
    }

    fn same_pcm(a: &BankPcm, b: &BankPcm) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.rate == b.rate
                        && a.channels.len() == b.channels.len()
                        && a.channels.iter().zip(&b.channels).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits()))
                }
                _ => false,
            })
    }

    #[test]
    fn unknown_failed_and_dropped_banks_fall_back_to_the_game_thread() {
        let mut p = Prefetch::default();
        assert!(p.take("nothing").is_none());
        let missing = BankSource::for_test(std::env::temp_dir().join("skate-prefetch-missing"), "gone", "aems/gone.abk", vec![]);
        // The game thread's own load reports the error it always did.
        assert!(missing.load().unwrap_err().starts_with("aems/gone.abk: "));
        p.request(missing.clone());
        p.request(missing.clone());
        assert_eq!(p.stems().count(), 1, "one slot per bank");
        p.wait("gone");
        assert!(p.take("gone").is_none(), "a failed load is retried (and reported) by the caller");
        assert!(!p.contains("gone"));
        p.request(missing.clone());
        p.drop_bank("gone");
        assert!(!p.contains("gone") && p.take("gone").is_none());
        p.request(missing);
        let clears = p.clears();
        p.clear();
        assert_eq!(p.stems().count(), 0);
        assert_eq!(p.clears(), clears + 1, "a clear is counted (the world sources re-request after one)");
    }

    /// The world emitter banks of a district (data-gated): the prefetched bank and PCM equal the
    /// game thread's own load bit for bit; `ensure_bank` then decodes nothing on the calling thread
    /// and takes far less time; a map change (`unload_map_banks`) forgets the prefetch and the next
    /// start loads on the game thread as before.
    #[test]
    #[ignore = "needs the private install data"]
    fn prefetched_banks_are_identical_and_not_decoded_at_the_start() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        if library.aems().projects.is_empty() {
            panic!("missing private data: no AEMS banks");
        }
        let mut stems: Vec<String> = Vec::new();
        for file in ["sfx_downtown", "sfx_university"] {
            for r in library.emitters(file) {
                if let Some(bank) = &r.bank {
                    if r.kind == 1 && r.flags == 0 && library.aems().banks.contains_key(bank) && !stems.contains(bank) {
                        stems.push(bank.clone());
                    }
                }
            }
        }
        if stems.is_empty() {
            panic!("missing private data: no emitter records");
        }
        // Identity: worker result vs the game thread's load.
        let mut p = Prefetch::default();
        for stem in &stems {
            p.request(library.bank_source(stem).unwrap());
        }
        for stem in &stems {
            p.wait(stem);
            let (bank, pcm) = p.take(stem).expect("prefetched");
            let (want_bank, want_pcm) = library.bank_source(stem).unwrap().load().unwrap();
            assert_eq!(format!("{bank:?}"), format!("{want_bank:?}"), "{stem}: bank");
            assert!(same_pcm(&pcm, &want_pcm), "{stem}: pcm");
            assert!(same_pcm(&pcm, &library.bank_pcm(stem)), "{stem}: pcm = Library::bank_pcm");
        }

        let mut native = Native::start(&library).expect("native start");
        let time = |native: &mut Native, stem: &str| {
            let before = decodes();
            let t = Instant::now();
            native.ensure_bank(&library, stem).unwrap();
            (t.elapsed().as_secs_f64() * 1e3, decodes() - before)
        };
        // Game-thread loads (the old path), warm disk cache.
        let mut sync = Vec::new();
        for stem in &stems {
            let (ms, n) = time(&mut native, stem);
            assert_eq!(n as usize, library.bank_pcm(stem).len(), "{stem}: decoded here");
            sync.push(ms);
        }
        // Map change: everything map-side goes, prefetched or loaded.
        for stem in &stems[..3] {
            native.prefetch.request(library.bank_source(stem).unwrap());
        }
        native.unload_map_banks();
        assert_eq!(native.prefetch.stems().count(), 0, "a map change forgets the prefetch");
        assert!(stems.iter().all(|s| !native.bank_loaded(s)));
        // Prefetched, then started.
        for stem in &stems {
            native.prefetch.request(library.bank_source(stem).unwrap());
        }
        for stem in &stems {
            native.prefetch.wait(stem);
        }
        let mut pre = Vec::new();
        for stem in &stems {
            let (ms, n) = time(&mut native, stem);
            assert_eq!(n, 0, "{stem}: no decode on the game thread");
            assert!(native.bank_loaded(stem));
            pre.push(ms);
        }
        let stat = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            format!("median {:.3} ms, max {:.3} ms", v[v.len() / 2], v[v.len() - 1])
        };
        // Timings are printed, not asserted (wall-clock order is the machine's, not the code's; the
        // decode counts above are the proof).
        println!("ensure_bank over {} emitter banks: game-thread load {}; prefetched {}", stems.len(), stat(&mut sync), stat(&mut pre));
    }
}
