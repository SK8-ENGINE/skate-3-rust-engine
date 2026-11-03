//! Opt-in per-system cost readout (`SKATE_SYSTEM_TIMING=1`, `SKATE_FIXED_TIMING=1`; b214 / b268).
//! Details: docs/hails-additions/25-perf-diagnostics.md#system-timing
use bevy::log::tracing::{Id, Subscriber, field::Field, field::Visit, span::Attributes};
use bevy::log::tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};
use bevy::prelude::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Whether the readout is on, read once.
pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKATE_SYSTEM_TIMING").is_ok_and(|v| v == "1"))
}

/// Whether the fixed-step readout is on, read once.
pub(crate) fn fixed_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKATE_FIXED_TIMING").is_ok_and(|v| v == "1"))
}

/// The span name of the fixed step (`bevy_app` `FixedMain::run_fixed_main`).
const FIXED_STEP: &str = "FixedMain::run_fixed_main";
/// Command applications get their own slot under this prefix (`system_commands` spans).
const COMMANDS: &str = "cmd:";

/// Systems named so far; a slot is never reused.
const SLOTS: usize = 8192;
/// Systems on the line.
const TOP: usize = 16;
/// A single run this long also lists the system after the top ones (`slow`), whatever its rank.
const SLOW_NS: u64 = 10_000_000;

struct Slot {
    total_ns: AtomicU64,
    runs: AtomicU64,
    max_ns: AtomicU64,
    /// Inside the fixed step only (`SKATE_FIXED_TIMING`).
    fixed_ns: AtomicU64,
    fixed_runs: AtomicU64,
    fixed_max_ns: AtomicU64,
    /// A system of a fixed schedule (set by `report_fixed` from `Schedules`).
    member: AtomicBool,
}

impl Slot {
    fn new() -> Self {
        let z = || AtomicU64::new(0);
        Self { total_ns: z(), runs: z(), max_ns: z(), fixed_ns: z(), fixed_runs: z(), fixed_max_ns: z(), member: AtomicBool::new(false) }
    }
}

/// The fixed step window and its totals since the last line.
static IN_FIXED: AtomicBool = AtomicBool::new(false);
static STEP_NS: AtomicU64 = AtomicU64::new(0);
static STEP_MAX_NS: AtomicU64 = AtomicU64::new(0);
static STEPS: AtomicU64 = AtomicU64::new(0);
/// Member spans open inside the window, when the count last left 0, and the summed time it was above 0.
static RUNNING: AtomicU32 = AtomicU32::new(0);
static BUSY_FROM: AtomicU64 = AtomicU64::new(0);
static BUSY_NS: AtomicU64 = AtomicU64::new(0);

fn now_ns() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

struct Table {
    names: Mutex<Vec<String>>,
    slots: Vec<Slot>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| Table {
        names: Mutex::new(Vec::new()),
        slots: (0..SLOTS).map(|_| Slot::new()).collect(),
    })
}

/// The span's slot, kept in its extensions.
struct SlotId(usize);
/// The fixed step's span (`FIXED_STEP`).
struct StepMarker;

#[derive(Default)]
struct NameField(Option<String>);
impl Visit for NameField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "name" {
            self.0 = Some(value.to_owned());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "name" && self.0.is_none() {
            self.0 = Some(format!("{value:?}").trim_matches('"').to_owned());
        }
    }
}

thread_local! {
    /// Open spans: id, enter time, entered inside the fixed step, counted as busy.
    static OPEN: RefCell<Vec<(u64, Instant, bool, bool)>> = const { RefCell::new(Vec::new()) };
}

pub(crate) struct SystemTimingLayer;

/// The layer when the readout is on.
pub(crate) fn layer() -> Option<SystemTimingLayer> {
    (on() || fixed_on()).then_some(SystemTimingLayer)
}

impl<S> Layer<S> for SystemTimingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let commands = match attrs.metadata().name() {
            "system" => false,
            "system_commands" if fixed_on() => true,
            _ => return,
        };
        let mut field = NameField::default();
        attrs.record(&mut field);
        let Some(name) = field.0 else { return };
        let step = !commands && name.ends_with(FIXED_STEP);
        let name = if commands { format!("{COMMANDS}{name}") } else { name };
        let t = table();
        let mut names = t.names.lock().unwrap_or_else(|e| e.into_inner());
        let slot = match names.iter().position(|n| *n == name) {
            Some(i) => i,
            None if names.len() < SLOTS => {
                names.push(name);
                names.len() - 1
            }
            None => {
                static FULL: std::sync::Once = std::sync::Once::new();
                FULL.call_once(|| warn!("SYSTEM_TIMING: all {SLOTS} name slots used; later systems are not timed"));
                return;
            }
        };
        drop(names);
        if let Some(span) = ctx.span(id) {
            let mut ext = span.extensions_mut();
            ext.insert(SlotId(slot));
            if step {
                ext.insert(StepMarker);
            }
        }
    }
    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let (mut inside, mut busy) = (false, false);
        if fixed_on() {
            if let Some(span) = ctx.span(id) {
                let ext = span.extensions();
                if ext.get::<StepMarker>().is_some() {
                    IN_FIXED.store(true, Ordering::Relaxed);
                } else if IN_FIXED.load(Ordering::Relaxed) {
                    inside = true;
                    busy = ext.get::<SlotId>().is_some_and(|s| table().slots[s.0].member.load(Ordering::Relaxed));
                    if busy && RUNNING.fetch_add(1, Ordering::Relaxed) == 0 {
                        BUSY_FROM.store(now_ns(), Ordering::Relaxed);
                    }
                }
            }
        }
        OPEN.with(|open| open.borrow_mut().push((id.into_u64(), Instant::now(), inside, busy)));
    }
    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let key = id.into_u64();
        let Some((begin, inside, busy)) = OPEN.with(|open| {
            let mut open = open.borrow_mut();
            let at = open.iter().rposition(|o| o.0 == key)?;
            let o = open.remove(at);
            Some((o.1, o.2, o.3))
        }) else {
            return;
        };
        if busy && RUNNING.fetch_sub(1, Ordering::Relaxed) == 1 {
            BUSY_NS.fetch_add(now_ns().saturating_sub(BUSY_FROM.load(Ordering::Relaxed)), Ordering::Relaxed);
        }
        let Some(span) = ctx.span(id) else { return };
        let ext = span.extensions();
        let Some(slot) = ext.get::<SlotId>().map(|s| s.0) else { return };
        let ns = begin.elapsed().as_nanos() as u64;
        let s = &table().slots[slot];
        s.total_ns.fetch_add(ns, Ordering::Relaxed);
        s.runs.fetch_add(1, Ordering::Relaxed);
        s.max_ns.fetch_max(ns, Ordering::Relaxed);
        if fixed_on() && ext.get::<StepMarker>().is_some() {
            IN_FIXED.store(false, Ordering::Relaxed);
            STEP_NS.fetch_add(ns, Ordering::Relaxed);
            STEP_MAX_NS.fetch_max(ns, Ordering::Relaxed);
            STEPS.fetch_add(1, Ordering::Relaxed);
        } else if inside {
            s.fixed_ns.fetch_add(ns, Ordering::Relaxed);
            s.fixed_runs.fetch_add(1, Ordering::Relaxed);
            s.fixed_max_ns.fetch_max(ns, Ordering::Relaxed);
        }
    }
}

pub(crate) struct SystemTimingPlugin;

impl Plugin for SystemTimingPlugin {
    fn build(&self, app: &mut App) {
        if on() {
            app.add_systems(Last, (report, count_material_events));
        }
        if fixed_on() {
            app.add_systems(Last, report_fixed);
        }
    }
}

#[derive(Default)]
struct ReportState {
    since: Option<Instant>,
    frames: u64,
}

/// Once per second: the `SYSTEM_TIMING` line, then the counters start again.
fn report(mut state: Local<ReportState>) {
    let now = Instant::now();
    let since = *state.since.get_or_insert(now);
    state.frames += 1;
    if now.duration_since(since).as_secs_f32() < 1.0 {
        return;
    }
    let frames = std::mem::take(&mut state.frames).max(1);
    state.since = Some(now);
    let t = table();
    let names = t.names.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut rows: Vec<(u64, u64, u64, &str)> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let s = &t.slots[i];
            (s.total_ns.swap(0, Ordering::Relaxed), s.runs.swap(0, Ordering::Relaxed), s.max_ns.swap(0, Ordering::Relaxed), n.as_str())
        })
        .filter(|r| r.1 > 0 && !r.3.starts_with(COMMANDS))
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let total: u64 = rows.iter().map(|r| r.0).sum();
    let top: Vec<String> = rows
        .iter()
        .take(TOP)
        .map(|(ns, runs, max, name)| format!("{}={:.3}/{:.2}x{}", short(name), *ns as f64 / 1e6 / frames as f64, *max as f64 / 1e6, runs))
        .collect();
    // Any system with one run of at least SLOW_NS, ranked or not: a single long wait (a swapchain acquire, a blocked
    // task picked up by a waiting scope) is small per second and stays out of the top list.
    let mut slow: Vec<&(u64, u64, u64, &str)> = rows.iter().filter(|r| r.2 >= SLOW_NS).collect();
    slow.sort_by(|a, b| b.2.cmp(&a.2));
    let slow: Vec<String> = slow.iter().map(|(_, _, max, name)| format!("{}={:.2}", short(name), *max as f64 / 1e6)).collect();
    info!(
        "SYSTEM_TIMING frames={frames} systems_ms_per_frame={:.3} | {}{}",
        total as f64 / 1e6 / frames as f64,
        top.join(" "),
        if slow.is_empty() { String::new() } else { format!(" | slow {}", slow.join(" ")) }
    );
    let living = crate::living_world::timing::take_second();
    let parts: Vec<String> = crate::living_world::timing::NAMES.iter().zip(living).map(|(n, us)| format!("{n}={:.3}", us as f64 / 1e3 / frames as f64)).collect();
    let phases = crate::frame_timing::hitch::take_phase_second();
    let parts_p: Vec<String> = crate::frame_timing::hitch::PHASES.iter().zip(phases).map(|(n, us)| format!("{n}={:.3}", us as f64 / 1e3 / frames as f64)).collect();
    info!("PHYSICS_PHASES frames={frames} | {}", parts_p.join(" "));
    info!("LIVING_TIMING frames={frames} ms_per_frame={:.3} | {}", living.iter().sum::<u64>() as f64 / 1e3 / frames as f64, parts.join(" "));
}

#[derive(Default)]
struct FixedReport {
    since: Option<Instant>,
    /// The fixed schedules' system count when membership was last read.
    members_for: usize,
}

/// The fixed schedules `FixedMain::run_fixed_main` runs, in order.
fn fixed_schedules() -> [bevy::ecs::intern::Interned<dyn bevy::ecs::schedule::ScheduleLabel>; 5] {
    use bevy::ecs::schedule::ScheduleLabel;
    [FixedFirst.intern(), FixedPreUpdate.intern(), FixedUpdate.intern(), FixedPostUpdate.intern(), FixedLast.intern()]
}

/// Once per second: the `FIXED_TIMING` line (every fixed-schedule system per step), then the counters start again.
fn report_fixed(schedules: Res<Schedules>, mut state: Local<FixedReport>) {
    let t = table();
    // Membership: the names of the fixed schedules' systems, read again whenever their count changes.
    let labels = fixed_schedules();
    let count: usize = labels.iter().filter_map(|l| schedules.get(*l)).map(|s| s.systems_len()).sum();
    if count != state.members_for {
        let members: Vec<String> = labels
            .iter()
            .filter_map(|l| schedules.get(*l))
            .filter_map(|s| s.systems().ok().map(|it| it.map(|(_, system)| system.name().to_string()).collect::<Vec<_>>()))
            .flatten()
            .collect();
        // Not initialised yet: try again next frame.
        if !members.is_empty() {
            let names = t.names.lock().unwrap_or_else(|e| e.into_inner());
            for (i, n) in names.iter().enumerate() {
                let plain = n.strip_prefix(COMMANDS).unwrap_or(n);
                t.slots[i].member.store(members.iter().any(|m| m == plain), Ordering::Relaxed);
            }
            state.members_for = count;
        }
    }
    let now = Instant::now();
    let since = *state.since.get_or_insert(now);
    if now.duration_since(since).as_secs_f32() < 1.0 {
        return;
    }
    state.since = Some(now);
    let steps = STEPS.swap(0, Ordering::Relaxed);
    let (step_ns, step_max) = (STEP_NS.swap(0, Ordering::Relaxed), STEP_MAX_NS.swap(0, Ordering::Relaxed));
    let busy_ns = BUSY_NS.swap(0, Ordering::Relaxed);
    let names = t.names.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut rows: Vec<(u64, u64, u64, &str, bool)> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let s = &t.slots[i];
            (s.fixed_ns.swap(0, Ordering::Relaxed), s.fixed_runs.swap(0, Ordering::Relaxed), s.fixed_max_ns.swap(0, Ordering::Relaxed), n.as_str(), s.member.load(Ordering::Relaxed))
        })
        .filter(|r| r.1 > 0)
        .collect();
    if steps == 0 {
        return;
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let per = |ns: u64| ns as f64 / 1e6 / steps as f64;
    let sum = |keep: &dyn Fn(&(u64, u64, u64, &str, bool)) -> bool| rows.iter().filter(|r| keep(r)).map(|r| r.0).sum::<u64>();
    let cpu = sum(&|r| r.4 && !r.3.starts_with(COMMANDS));
    let commands = sum(&|r| r.4 && r.3.starts_with(COMMANDS));
    let unlisted = sum(&|r| !r.4);
    let systems: Vec<String> = rows
        .iter()
        .filter(|r| r.4 && (!r.3.starts_with(COMMANDS) || per(r.0) >= 0.01))
        .map(|(ns, runs, max, name, _)| {
            let (prefix, plain) = name.strip_prefix(COMMANDS).map_or(("", *name), |p| (COMMANDS, p));
            format!("{prefix}{}={:.3}/{:.2}x{}", short(plain), per(*ns), *max as f64 / 1e6, runs)
        })
        .collect();
    let others: Vec<String> = rows.iter().filter(|r| !r.4).take(3).map(|(ns, runs, _, name, _)| format!("{}={:.2}x{}", short(name), per(*ns), runs)).collect();
    info!(
        "FIXED_TIMING steps={steps} step_ms={:.2}/{:.2} busy_ms={:.2} gaps_ms={:.2} cpu_ms={:.2} commands_ms={:.2} unlisted_ms={:.2} | {}{}",
        per(step_ns),
        step_max as f64 / 1e6,
        per(busy_ns),
        per(step_ns.saturating_sub(busy_ns)),
        per(cpu),
        per(commands),
        per(unlisted),
        systems.join(" "),
        if others.is_empty() { String::new() } else { format!(" | unlisted {}", others.join(" ")) }
    );
}

/// The last two path segments of a system name (`living_world::peds::think_peds` -> `peds::think_peds`), generics kept.
fn short(name: &str) -> &str {
    let plain = name.split('<').next().unwrap_or(name);
    let mut cut = 0;
    let mut seen = 0;
    for (i, _) in plain.match_indices("::").collect::<Vec<_>>().into_iter().rev() {
        seen += 1;
        if seen == 2 {
            cut = i + 2;
            break;
        }
    }
    &name[cut..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_keep_the_last_two_segments() {
        assert_eq!(short("skate3rust::living_world::peds::think_peds"), "peds::think_peds");
        assert_eq!(short("think"), "think");
        assert_eq!(short("bevy_pbr::material::check<skate3rust::a::B>"), "material::check<skate3rust::a::B>");
    }

    /// FIXED_TIMING finds the fixed step by the name of the one system in `FixedMain` (b268: the exact text was
    /// not verified from source alone).
    #[test]
    fn the_fixed_step_system_is_found_by_name() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let world = app.world_mut();
        // Out of `Schedules` while it initializes: initializing inside `resource_scope` re-inserts `Schedules`, which
        // bevy 0.18 rejects.
        let mut fixed = world.resource_mut::<Schedules>().remove(bevy::app::FixedMain).unwrap();
        fixed.initialize(world).unwrap();
        world.resource_mut::<Schedules>().insert(fixed);
        let schedules = app.world().resource::<Schedules>();
        let names: Vec<String> = schedules.get(bevy::app::FixedMain).unwrap().systems().unwrap().map(|(_, s)| s.name().to_string()).collect();
        assert_eq!(names.iter().filter(|n| n.ends_with(FIXED_STEP)).count(), 1, "{names:?}");
    }
}

/// Added / modified material assets per second, per material type (`MATERIAL_EVENTS`): a material written every frame
/// makes the render world rebuild its bind group every frame (`prepare_material_bind_groups`).
#[allow(clippy::too_many_arguments)]
fn count_material_events(
    mut ped: MessageReader<AssetEvent<crate::living_world::ped_material::PedMaterial>>,
    mut vehicle: MessageReader<AssetEvent<crate::living_world::vehicle_material::VehicleMaterial>>,
    mut character: MessageReader<AssetEvent<crate::retail_character::CharacterMaterial>>,
    mut world: MessageReader<AssetEvent<crate::retail_render::WorldMaterial>>,
    mut hud: MessageReader<AssetEvent<crate::scoring_hud::HudMaterial>>,
    mut standard: MessageReader<AssetEvent<StandardMaterial>>,
    mut colour: MessageReader<AssetEvent<ColorMaterial>>,
    mut images: MessageReader<AssetEvent<Image>>,
    mut state: Local<(Option<Instant>, [[u64; 2]; 8])>,
) {
    fn tally<A: Asset>(r: &mut MessageReader<AssetEvent<A>>, c: &mut [u64; 2]) {
        for e in r.read() {
            match e {
                AssetEvent::Added { .. } => c[0] += 1,
                AssetEvent::Modified { .. } => c[1] += 1,
                _ => {}
            }
        }
    }
    let c = &mut state.1;
    tally(&mut ped, &mut c[0]);
    tally(&mut vehicle, &mut c[1]);
    tally(&mut character, &mut c[2]);
    tally(&mut world, &mut c[3]);
    tally(&mut hud, &mut c[4]);
    tally(&mut standard, &mut c[5]);
    tally(&mut colour, &mut c[6]);
    tally(&mut images, &mut c[7]);
    let now = Instant::now();
    let since = *state.0.get_or_insert(now);
    if now.duration_since(since).as_secs_f32() < 1.0 {
        return;
    }
    state.0 = Some(now);
    let names = ["ped", "vehicle", "character", "world", "hud", "standard", "colour", "image"];
    let parts: Vec<String> = names.iter().zip(std::mem::take(&mut state.1)).map(|(n, [a, m])| format!("{n}={a}+{m}")).collect();
    info!("MATERIAL_EVENTS added+modified per second | {}", parts.join(" "));
}
