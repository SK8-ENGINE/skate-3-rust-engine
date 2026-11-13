//! `SKATE_FRAME_LOG=<file>`: one tab-separated row per rendered frame, written
//! by a background thread. Without the variable nothing is created and the
//! game thread does no extra work.
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    },
    time::Duration,
};

pub(crate) const ENV: &str = "SKATE_FRAME_LOG";
/// First line of every log; bump the version when a column changes.
pub(crate) const MAGIC: &str = "# skate3rust frame log v1";
pub(crate) const HEADER: &str = "wall_unix_s\tframe\tframe_ms\tfixed_steps\tfixed_ms\tmain_ms\thitch\tmedian_ms";
/// Maximum idle wait between writer checks.
const FLUSH_EVERY: Duration = Duration::from_millis(250);
/// Bound memory when storage cannot keep up; never stall the game thread.
const QUEUE_CAPACITY: usize = 4096;

/// One rendered frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Row {
    /// Wall clock (UTC, seconds since the Unix epoch) when the frame was recorded,
    /// to line rows up with the game log and the audio state log.
    pub wall_unix_s: f64,
    /// Rendered-frame counter since start (the first recorded frame is 1).
    pub frame: u64,
    /// Real time since the previous frame started, in ms.
    pub frame_ms: f32,
    /// Fixed-update (physics) steps run in this frame; > 1 = catching up.
    pub fixed_steps: u32,
    /// CPU time of those fixed steps (the whole fixed loop), in ms.
    pub fixed_ms: f32,
    /// CPU time of the main world's schedules this frame (First..Last), in ms.
    /// Much less than `frame_ms` = the frame waited for rendering / the GPU / present.
    pub main_ms: f32,
    /// Longer than 2x the frame's expected time (see `stats::expected_ms`).
    pub hitch: bool,
    /// That expected time (column kept as `median_ms`: before 2026-10-19 it was the median of all previous frames);
    /// empty until enough history exists.
    pub median_ms: Option<f32>,
}

/// Formats one row (no trailing newline).
pub(crate) fn format_row(row: &Row) -> String {
    let median = row.median_ms.map_or(String::new(), |m| format!("{m:.3}"));
    format!(
        "{:.3}\t{}\t{:.3}\t{}\t{:.3}\t{:.3}\t{}\t{}",
        row.wall_unix_s,
        row.frame,
        row.frame_ms,
        row.fixed_steps,
        row.fixed_ms,
        row.main_ms,
        if row.hitch { "HITCH" } else { "" },
        median
    )
}

/// Parses a row written by [`format_row`] (tools and tests).
#[cfg(test)]
pub(crate) fn parse_row(line: &str) -> Option<Row> {
    let mut fields = line.split('\t');
    let row = Row {
        wall_unix_s: fields.next()?.parse().ok()?,
        frame: fields.next()?.parse().ok()?,
        frame_ms: fields.next()?.parse().ok()?,
        fixed_steps: fields.next()?.parse().ok()?,
        fixed_ms: fields.next()?.parse().ok()?,
        main_ms: fields.next()?.parse().ok()?,
        hitch: match fields.next()? {
            "HITCH" => true,
            "" => false,
            _ => return None,
        },
        median_ms: match fields.next()? {
            "" => None,
            value => Some(value.parse().ok()?),
        },
    };
    fields.next().is_none().then_some(row)
}

/// Sends rows to the writer thread. Dropping it flushes and joins the thread.
pub(crate) struct FrameLog {
    sender: Option<SyncSender<Row>>,
    dropped: AtomicU64,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for FrameLog {
    fn drop(&mut self) {
        drop(self.sender.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let dropped = self.dropped.load(Ordering::Relaxed);
        if dropped != 0 {
            eprintln!("{ENV}: dropped {dropped} rows because the frame-log queue was full");
        }
    }
}

impl FrameLog {
    /// Opens the log named by [`ENV`]; `None` when unset or empty.
    pub(crate) fn from_env() -> Option<Self> {
        let path = std::env::var_os(ENV).filter(|p| !p.is_empty())?;
        match Self::open(PathBuf::from(path)) {
            Ok(log) => Some(log),
            Err(error) => {
                bevy::log::warn!("{ENV}: {error}");
                None
            }
        }
    }

    pub(crate) fn open(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(&path)?;
        let (sender, receiver) = mpsc::sync_channel::<Row>(QUEUE_CAPACITY);
        let thread = std::thread::Builder::new()
            .name("frame-log".into())
            .spawn(move || {
                let mut out = std::io::BufWriter::new(file);
                let mut ok = writeln!(out, "{MAGIC}").and_then(|_| writeln!(out, "{HEADER}")).is_ok();
                loop {
                    match receiver.recv_timeout(FLUSH_EVERY) {
                        Ok(row) => {
                            ok &= writeln!(out, "{}", format_row(&row)).is_ok();
                            // Drain what is queued, then flush once.
                            while let Ok(row) = receiver.try_recv() {
                                ok &= writeln!(out, "{}", format_row(&row)).is_ok();
                            }
                            ok &= out.flush().is_ok();
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                let _ = out.flush();
                if !ok {
                    eprintln!("{ENV}: writing the frame log failed");
                }
            })?;
        bevy::log::info!("Frame log: {}", path.display());
        Ok(Self { sender: Some(sender), dropped: AtomicU64::new(0), thread: Some(thread) })
    }

    /// Never blocks; a full queue drops the row, and a closed writer is ignored.
    pub(crate) fn send(&self, row: Row) {
        if let Some(sender) = &self.sender {
            if let Err(TrySendError::Full(_)) = sender.try_send(row) {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(frame: u64, hitch: bool, median_ms: Option<f32>) -> Row {
        Row { wall_unix_s: 1_759_622_400.125, frame, frame_ms: 16.667, fixed_steps: 2, fixed_ms: 1.5, main_ms: 4.25, hitch, median_ms }
    }

    #[test]
    fn a_stalled_writer_has_a_bounded_queue_and_does_not_block() {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let log = FrameLog { sender: Some(sender), dropped: AtomicU64::new(0), thread: None };
        for frame in 0..QUEUE_CAPACITY as u64 + 3 {
            log.send(row(frame, false, None));
        }
        assert_eq!(log.dropped.load(Ordering::Relaxed), 3);
        let queued: Vec<_> = receiver.try_iter().collect();
        assert_eq!(queued.len(), QUEUE_CAPACITY);
        assert_eq!(queued.last().unwrap().frame, QUEUE_CAPACITY as u64 - 1);
        drop(receiver);
        log.send(row(9999, false, None));
        assert_eq!(log.dropped.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn rows_have_a_fixed_column_format() {
        assert_eq!(format_row(&row(7, false, None)), "1759622400.125\t7\t16.667\t2\t1.500\t4.250\t\t");
        assert_eq!(format_row(&row(8, true, Some(6.1))), "1759622400.125\t8\t16.667\t2\t1.500\t4.250\tHITCH\t6.100");
        assert_eq!(HEADER.split('\t').count(), format_row(&row(1, false, None)).split('\t').count());
    }

    #[test]
    fn rows_round_trip() {
        for r in [row(1, false, None), row(2, true, Some(4.25))] {
            assert_eq!(parse_row(&format_row(&r)), Some(r));
        }
        assert_eq!(parse_row("1\t2\t3\t4\t5\t6\tmaybe\t"), None);
        assert_eq!(parse_row("1\t2\t3\t4\t5\t6\t\t\textra"), None);
        assert_eq!(parse_row("1\t2\t3"), None);
    }

    #[test]
    fn writer_thread_writes_header_and_rows_then_flushes_on_drop() {
        let dir = std::env::temp_dir().join(format!("skate-frame-log-{}", std::process::id()));
        let path = dir.join("nested").join("frames.tsv");
        let log = FrameLog::open(path.clone()).unwrap();
        for frame in 1..=500 {
            log.send(row(frame, frame % 100 == 0, Some(5.0)));
        }
        // Dropping flushes and joins the writer, so everything is on disk now.
        drop(log);
        let text = std::fs::read_to_string(&path).unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some(MAGIC));
        assert_eq!(lines.next(), Some(HEADER));
        let rows: Vec<Row> = lines.map(|l| parse_row(l).expect(l)).collect();
        assert_eq!(rows.len(), 500);
        assert!(rows.iter().enumerate().all(|(i, r)| r.frame == i as u64 + 1));
        assert_eq!(rows.iter().filter(|r| r.hitch).count(), 5);
        let _ = std::fs::remove_dir_all(dir);
    }
}
