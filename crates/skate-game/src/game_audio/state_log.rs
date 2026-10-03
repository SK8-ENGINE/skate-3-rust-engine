//! `SKATE_AUDIO_STATE_LOG=<path>`: record the player's per-frame audio situation while playing, as
//! one TSV row per physics/audio frame in the columns of the e2e scenarios
//! (`tools/audio-e2e/scenarios.py`) plus the elapsed time and the board / COM
//! world positions, so real play can be cut into windows and replayed headless into our native
//! stack and the PoC oracle (`scenarios.py --from-log`). Off (no cost) when unset; buffered,
//! flushed every 6 rows (~100 ms).
use std::io::Write;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// The header (the e2e scenario columns between `ms` and the positions).
pub(crate) const HEADER: &str = "ms\tframe\tspeed\tturn\twheels\ttag\tair\tair_time\tto_land\tjump_height\tjv\tgrinding\tfamily\tgrind_tag\tbrake\tmanual\tbalance\tscorable\tpush\tslope\ttilt\tslip\tfeet\tstate\tboard_x\tboard_y\tboard_z\tcom_x\tcom_y\tcom_z\tgrind_impact\tdeck_impact\tdeck_tag\tfoot_y0\tfoot_y1\tfoot_xz0\tfoot_xz1\tseam0\tseam3\twheel_x\twheel_z\theading\tlines\tfoot_down\tfoot_tag_a\tfoot_tag_b\thands\tstrength\tfoot_vy_a\tfoot_vy_b\tstep\tbody\tlimb\tslide\tdeck_up\tdeck_contact\tplant\tstroke\tdeck_spin\tspin_x\tspin_y\tbail\tbail_end\theld\toffboard_air\tfootplant\trevert\tsoft\tface\trimp0\trimp1\trimp2\trimp3\trimp4\trimp5\trslide0\trslide1\trslide2\trslide3\trslide4\trslide5\trtag0\trtag1\trtag2\trtag3\trtag4\trtag5\tcom_speed";

/// One frame of the log.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Row {
    pub ms: f64,
    pub frame: u64,
    pub speed: f32,
    pub turn: f32,
    /// Bit i = wheel i in contact.
    pub wheels: u32,
    pub tag: u32,
    pub air: bool,
    pub air_time: f32,
    pub to_land: f32,
    pub jump_height: f32,
    /// |Air+112| (m/s) on frames where Air440 holds, else 0.
    pub jv: f32,
    pub grinding: bool,
    pub family: i32,
    pub grind_tag: u32,
    pub brake: bool,
    pub manual: bool,
    pub balance: bool,
    pub scorable: i32,
    pub push: bool,
    pub slope: f32,
    pub tilt: f32,
    /// Lateral board speed / board speed (the sine the e2e probes turn the deck by).
    pub slip: f32,
    /// Bit 0 / 1: feet in the deck box.
    pub feet: u32,
    pub state: u32,
    pub board: [f32; 3],
    pub com: [f32; 3],
    /// Audio state `+228` (last grind impact), `+668` (deck impact), the deck contact's tag, and
    /// the toes' local speeds (`+272`/`+268` Y, `+280`/`+276` XZ) — appended 2026-10-02.
    pub grind_impact: f32,
    pub deck_impact: f32,
    pub deck_tag: u32,
    pub foot_y: [f32; 2],
    pub foot_xz: [f32; 2],
    /// The seam patterns of wheels 0 and 3 (`+636` / `+648`), wheel 0's world x, z and the deck's
    /// heading (atan2 of its At axis, x over z, rad) — appended for Class_Seams replays.
    pub seam: [u32; 2],
    pub wheel: [f32; 2],
    pub heading: f32,
    /// Wheels whose wheel line hits a surface (bit i = wheel i; `WheelLineState.audio_surfaces`
    /// non-zero): the audio record's materials come from these lines, not from contact.
    pub lines: u32,
    /// The off-board inputs (`player::footsteps` / `step_on`): feet down (bit 0 = A, 1 = B), the
    /// feet's materials (tag − 1, 143 none), hands on the deck (bits), the footstep strength and the
    /// feet's vertical speeds — appended for footstep replays.
    pub foot_down: u32,
    pub foot_material: [u32; 2],
    pub hands: u32,
    pub strength: f32,
    pub foot_vy: [f32; 2],
    /// The skeleton inputs (appended for clothing / footstep replays): the step code (`+740`), the
    /// body speed (`+328`), the limb speed (`+672`), the largest body-region slide speed
    /// (`+528..+548`); and the loose-board inputs: `up_dot` and the deck contact.
    pub step: i32,
    pub body: f32,
    pub limb: f32,
    pub slide: f32,
    pub deck_up: f32,
    pub deck_contact: bool,
    /// Appended 2026-10-03 (session review #1 / #10, the e2e harness): the push plant State55
    /// (`+333 || +334`, the push edge `+335` is its rise), the push stroke State56 (`+337`), the
    /// deck spin `+488` / `+480` / `+484`, bail `+676` / end `+677`, the board held `+308`,
    /// OffboardAir `+718`, the footplant `+768`, reverting `+690`, soft wheels `+684`, the face
    /// point's contact `+593`, and per body region 0..5 the impact `+496..`, slide speed `+528..`
    /// and surface tag `+560..`.
    pub plant: bool,
    pub stroke: bool,
    pub deck_spin: [f32; 3],
    pub bail: bool,
    pub bail_end: bool,
    pub held: bool,
    pub offboard_air: bool,
    pub footplant: bool,
    pub revert: bool,
    pub soft: bool,
    pub face: bool,
    pub region_impact: [f32; 6],
    pub region_slide: [f32; 6],
    pub region_tag: [u32; 6],
    /// Appended 2026-10-03: the bridge's `+212` |COM v| (m/s), the speed graph's input one row later
    /// (`+216`), so replays apply the graph to the logged (pre-graph) region impacts.
    pub com_speed: f32,
}

impl Row {
    pub(crate) fn line(&self) -> String {
        let b = |v: bool| u8::from(v);
        format!(
            "{:.1}\t{}\t{:.6}\t{:.6}\t{}\t{}\t{}\t{:.6}\t{:.6}\t{:.6}\t{:.6}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.6}\t{:.6}\t{:.6}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.6}\t{:.6}\t{}\t{:.6}\t{:.6}\t{:.6}\t{:.6}\t{}\t{}\t{:.4}\t{:.4}\t{:.5}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.5}",
            self.ms, self.frame, self.speed, self.turn, self.wheels, self.tag, b(self.air), self.air_time, self.to_land,
            self.jump_height, self.jv, b(self.grinding), self.family, self.grind_tag, b(self.brake), b(self.manual),
            b(self.balance), self.scorable, b(self.push), self.slope, self.tilt, self.slip, self.feet, self.state,
            self.board[0], self.board[1], self.board[2], self.com[0], self.com[1], self.com[2],
            self.grind_impact, self.deck_impact, self.deck_tag, self.foot_y[0], self.foot_y[1], self.foot_xz[0], self.foot_xz[1],
            self.seam[0], self.seam[1], self.wheel[0], self.wheel[1], self.heading, self.lines,
            self.foot_down, self.foot_material[0], self.foot_material[1], self.hands, self.strength, self.foot_vy[0], self.foot_vy[1],
            self.step, self.body, self.limb, self.slide, self.deck_up, b(self.deck_contact),
            b(self.plant), b(self.stroke), self.deck_spin[0], self.deck_spin[1], self.deck_spin[2], b(self.bail),
            b(self.bail_end), b(self.held), b(self.offboard_air), b(self.footplant), b(self.revert), b(self.soft), b(self.face),
            self.region_impact[0], self.region_impact[1], self.region_impact[2], self.region_impact[3], self.region_impact[4],
            self.region_impact[5], self.region_slide[0], self.region_slide[1], self.region_slide[2], self.region_slide[3],
            self.region_slide[4], self.region_slide[5], self.region_tag[0], self.region_tag[1], self.region_tag[2],
            self.region_tag[3], self.region_tag[4], self.region_tag[5], self.com_speed
        )
    }
}

/// The writer thread's queue (rows as text). The game thread only formats and enqueues: a disk
/// flush or OS stall must never block a frame (19:20 listening test: `observe` took 143–751 ms
/// on three frames while the flush ran on the game thread).
static QUEUE: OnceLock<Option<std::sync::mpsc::SyncSender<String>>> = OnceLock::new();
/// Rows dropped because the queue was full (reported by the writer at the end).
static DROPPED: AtomicU64 = AtomicU64::new(0);

fn queue() -> Option<&'static std::sync::mpsc::SyncSender<String>> {
    QUEUE
        .get_or_init(|| {
            let path = std::path::PathBuf::from(std::env::var_os("SKATE_AUDIO_STATE_LOG")?);
            let (tx, rx) = std::sync::mpsc::sync_channel::<String>(4096);
            std::thread::Builder::new()
                .name("audio-state-log".into())
                .spawn(move || {
                    let Ok(file) = std::fs::File::create(&path) else { return };
                    let mut out = std::io::BufWriter::with_capacity(1 << 16, file);
                    if writeln!(out, "{HEADER}").is_err() {
                        return;
                    }
                    bevy::log::info!("Game audio: audio state log → {}", path.display());
                    let mut rows = 0u32;
                    while let Ok(line) = rx.recv() {
                        if writeln!(out, "{line}").is_err() {
                            return;
                        }
                        rows += 1;
                        if rows % 6 == 0 {
                            let _ = out.flush();
                        }
                    }
                    let _ = out.flush();
                })
                .ok()?;
            Some(tx)
        })
        .as_ref()
}

/// Append a row: formatted here, written by the log thread (never blocks; a full queue drops the
/// row and counts it).
pub(crate) fn write(row: &Row) {
    let Some(tx) = queue() else { return };
    if tx.try_send(row.line()).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

/// Rows dropped so far (a full queue).
pub(crate) fn dropped() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}

/// Whether the log is requested (checked once per frame by the caller to skip building rows).
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SKATE_AUDIO_STATE_LOG").is_some_and(|v| !v.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_has_the_header_columns_in_order() {
        let row = Row { ms: 12.5, frame: 3, speed: 5.5, wheels: 15, tag: 3, air: true, family: -1, scorable: 128, state: 201, board: [1.0, 2.0, 3.0], ..Default::default() };
        let cols: Vec<&str> = HEADER.split('\t').collect();
        let line = row.line();
        let vals: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), vals.len());
        let get = |c: &str| vals[cols.iter().position(|x| *x == c).unwrap()];
        assert_eq!((get("ms"), get("frame"), get("wheels"), get("tag"), get("air")), ("12.5", "3", "15", "3", "1"));
        assert_eq!((get("family"), get("scorable"), get("state"), get("board_z")), ("-1", "128", "201", "3.0000"));
        assert_eq!(get("speed").parse::<f32>().unwrap(), 5.5);
        // The e2e scenario columns are a contiguous run of the header.
        let scenario = "frame speed turn wheels tag air air_time to_land jump_height jv grinding family grind_tag brake manual balance scorable push slope tilt slip feet state";
        assert_eq!(cols[1..24].join(" "), scenario);
    }
}
