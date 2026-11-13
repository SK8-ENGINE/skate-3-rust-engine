//! Frame-time statistics: pure functions over frame durations in milliseconds.
//!
//! Definitions (kept here so the readout, the log and the mod snapshot agree):
//! - **median**: the middle frame time (nearest rank, 50 %).
//! - **1 % low / 0.1 % low**: the frame time that the slowest 1 % / 0.1 % of
//!   frames reach or exceed — the nearest-rank 99th / 99.9th percentile of the
//!   frame times. Reported in ms (and as fps = 1000 / ms where a rate is shown).
//!   An average FPS hides a hitch; these do not.
//! - **worst**: the longest frame in the window.
//! - **hitch**: docs/hails-additions/25-perf-diagnostics.md#code-reference-apprs-performancers-frame_timingstatsrs

/// A frame is a hitch when it is longer than this many expected frame times.
pub(crate) const HITCH_FACTOR: f32 = 2.0;
/// Frames before the current one used for the hitch median.
pub(crate) const HITCH_HISTORY: usize = 120;
/// No hitch is reported until this many frames of history exist.
pub(crate) const HITCH_MIN_HISTORY: usize = 30;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Summary {
    pub frames: usize,
    pub mean_ms: f32,
    pub median_ms: f32,
    pub low_1_ms: f32,
    pub low_01_ms: f32,
    pub worst_ms: f32,
}

impl Summary {
    /// Mean frames per second over the window (0 when empty).
    pub(crate) fn fps(&self) -> f32 {
        if self.mean_ms > 0.0 { 1000.0 / self.mean_ms } else { 0.0 }
    }
}

/// Nearest-rank percentile of an ascending-sorted slice; `p` in (0, 100].
pub(crate) fn percentile(sorted: &[f32], p: f64) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    // The epsilon keeps exact ranks exact: 0.999 * 1000 is 999.0000000000001.
    let rank = (p * sorted.len() as f64 / 100.0 - 1e-9).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Summarises frame times. `scratch` is reused to avoid an allocation per call.
pub(crate) fn summarise(frames: impl Iterator<Item = f32>, scratch: &mut Vec<f32>) -> Summary {
    scratch.clear();
    scratch.extend(frames.filter(|ms| ms.is_finite() && *ms >= 0.0));
    if scratch.is_empty() {
        return Summary::default();
    }
    let sum: f64 = scratch.iter().map(|&ms| f64::from(ms)).sum();
    scratch.sort_unstable_by(f32::total_cmp);
    Summary {
        frames: scratch.len(),
        mean_ms: (sum / scratch.len() as f64) as f32,
        median_ms: percentile(scratch, 50.0),
        low_1_ms: percentile(scratch, 99.0),
        low_01_ms: percentile(scratch, 99.9),
        worst_ms: *scratch.last().unwrap(),
    }
}

/// Median of the history (nearest rank), or `None` with too little history.
pub(crate) fn history_median(history: impl Iterator<Item = f32>, scratch: &mut Vec<f32>) -> Option<f32> {
    scratch.clear();
    scratch.extend(history);
    if scratch.len() < HITCH_MIN_HISTORY {
        return None;
    }
    let middle = (scratch.len() + 1) / 2 - 1;
    let (_, median, _) = scratch.select_nth_unstable_by(middle, f32::total_cmp);
    Some(*median)
}

/// The time a frame running `steps` fixed steps is expected to take: the median frame without its physics loop
/// plus `steps` × the median cost of one step. `None` until both medians have history (the step median only matters
/// when `steps > 0`).
pub(crate) fn expected_ms(rest_median_ms: Option<f32>, step_median_ms: Option<f32>, steps: u32) -> Option<f32> {
    let rest = rest_median_ms?;
    if steps == 0 {
        return Some(rest);
    }
    Some(rest + steps as f32 * step_median_ms?)
}

/// The hitch rule: longer than [`HITCH_FACTOR`] × the expected frame time ([`expected_ms`]).
pub(crate) fn is_hitch(frame_ms: f32, median_ms: Option<f32>) -> bool {
    median_ms.is_some_and(|median| median > 0.0 && frame_ms > HITCH_FACTOR * median)
}

/// The worst frame per time bucket, oldest bucket first: `buckets` buckets of
/// `bucket_s` seconds ending at `now_s`. A frame belongs to the bucket that
/// contains its end time; empty buckets are 0. Used for the graph, so a hitch
/// always shows as one tall bar however many short frames surround it.
pub(crate) fn bucket_worst(
    samples: impl Iterator<Item = (f64, f32)>,
    now_s: f64,
    bucket_s: f64,
    out: &mut [f32],
) {
    out.fill(0.0);
    let buckets = out.len();
    if buckets == 0 || bucket_s <= 0.0 {
        return;
    }
    for (end_s, ms) in samples {
        let age = now_s - end_s;
        if !(age >= 0.0) {
            continue;
        }
        let back = (age / bucket_s) as usize;
        if back >= buckets {
            continue;
        }
        let slot = &mut out[buckets - 1 - back];
        if ms > *slot {
            *slot = ms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_nearest_rank() {
        let sorted: Vec<f32> = (1..=1000).map(|i| i as f32).collect();
        assert_eq!(percentile(&sorted, 50.0), 500.0);
        assert_eq!(percentile(&sorted, 99.0), 990.0);
        assert_eq!(percentile(&sorted, 99.9), 999.0);
        assert_eq!(percentile(&sorted, 100.0), 1000.0);
        assert_eq!(percentile(&[7.0], 99.9), 7.0);
        assert_eq!(percentile(&[], 50.0), 0.0);
        // Fewer than 100 frames: the 1 % low is the worst frame.
        let small: Vec<f32> = (1..=50).map(|i| i as f32).collect();
        assert_eq!(percentile(&small, 99.0), 50.0);
    }

    #[test]
    fn one_hitch_is_visible_where_an_average_hides_it() {
        // 0.5 s of 3 ms frames plus one 200 ms frame: the old counter's
        // 0.5 s average reads ~ (167 + 1) / 0.701 s = 240 fps.
        let mut frames = vec![3.0_f32; 167];
        frames.push(200.0);
        let mut scratch = Vec::new();
        let s = summarise(frames.iter().copied(), &mut scratch);
        assert_eq!(s.frames, 168);
        assert_eq!(s.worst_ms, 200.0);
        assert_eq!(s.low_01_ms, 200.0);
        assert_eq!(s.low_1_ms, 3.0);
        assert_eq!(s.median_ms, 3.0);
        assert!((s.fps() - 168.0 / 0.701).abs() < 0.5, "{}", s.fps());
    }

    #[test]
    fn summary_ignores_invalid_and_empty_input() {
        let mut scratch = Vec::new();
        assert_eq!(summarise(std::iter::empty(), &mut scratch), Summary::default());
        assert_eq!(Summary::default().fps(), 0.0);
        let s = summarise([f32::NAN, -1.0, f32::INFINITY, 4.0].into_iter(), &mut scratch);
        assert_eq!(s.frames, 1);
        assert_eq!(s.mean_ms, 4.0);
        assert_eq!(s.fps(), 250.0);
    }

    #[test]
    fn hitch_needs_history_and_twice_the_median() {
        let mut scratch = Vec::new();
        let short: Vec<f32> = vec![5.0; HITCH_MIN_HISTORY - 1];
        assert_eq!(history_median(short.iter().copied(), &mut scratch), None);
        assert!(!is_hitch(1000.0, None));
        let mut history: Vec<f32> = vec![5.0; 100];
        history.extend([50.0; 20]);
        let median = history_median(history.iter().copied(), &mut scratch);
        assert_eq!(median, Some(5.0));
        assert!(!is_hitch(10.0, median), "exactly 2x is not a hitch");
        assert!(is_hitch(10.001, median));
        assert!(!is_hitch(100.0, Some(0.0)));
        // Even history length: lower middle (nearest rank).
        let even: Vec<f32> = (1..=40).map(|i| i as f32).collect();
        assert_eq!(history_median(even.iter().copied(), &mut scratch), Some(20.0));
    }

    /// The laptop's 2026-10-19 pattern: ~5 ms frames, every other one running a ~6 ms fixed step (~11 ms). The step
    /// frames are expected, a real spike in either kind of frame still is a hitch.
    #[test]
    fn fixed_step_frames_are_expected_not_hitches() {
        let (rest, step) = (Some(5.1), Some(5.9));
        assert_eq!(expected_ms(rest, step, 0), Some(5.1));
        let one = expected_ms(rest, step, 1);
        assert!(!is_hitch(11.9, one), "a normal one-step frame");
        assert!(!is_hitch(13.1, one), "the session's p99 one-step frame");
        assert!(is_hitch(25.0, one));
        assert!(is_hitch(10.5, expected_ms(rest, step, 0)), "a step-less frame twice as long");
        assert_eq!(expected_ms(rest, None, 1), None, "no step history yet");
        assert_eq!(expected_ms(None, step, 0), None);
        // The old rule (2 x the median of all frames, ~5.6 ms) flagged the normal one-step frame.
        assert!(is_hitch(11.9, Some(5.6)));
    }

    #[test]
    fn buckets_keep_the_worst_frame_per_slot() {
        let mut out = [0.0_f32; 4];
        let samples = [(9.99, 3.0), (9.98, 8.0), (9.6, 120.0), (9.0, 4.0), (8.0, 99.0), (10.5, 1.0)];
        bucket_worst(samples.into_iter(), 10.0, 0.25, &mut out);
        // Newest bucket last: (9.75, 10] -> 8; (9.5, 9.75] -> 120; then 0;
        // 9.0 is exactly 1 s old -> bucket 4, outside; 8.0 outside; 10.5 future.
        assert_eq!(out, [0.0, 0.0, 120.0, 8.0]);
        let mut none: [f32; 0] = [];
        bucket_worst(samples.into_iter(), 10.0, 0.25, &mut none);
    }
}
