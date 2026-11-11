//! The launcher window (doc 44a section 2), on every OS: a page stack driven by a controller (gilrs), the keyboard or
//! the mouse. Every action runs this program's own text commands as a child process with `SKATE_LAUNCHER_UI=2`, the
//! way SkateLauncher.exe runs launcher.ps1, so the window and the command line share one backend.
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

const LABELS: &[&str] = &["session", "ride", "grind", "bail", "push", "emit", "carve", "water", "marker"];

#[derive(Clone, Default)]
struct Item {
    text: String,
    hint: String,
    on: bool,
    info: String,
    records: Vec<String>,
    action: Action,
}

#[derive(Clone, Default)]
enum Action {
    #[default]
    None,
    Open(PageKind),
    Begin { entry: String, ask: bool },
    BeginLabel { entry: String, label: String },
    Job(Vec<String>),
    SetVersion(String),
    Setting(String),
    Confirm { text: String, args: Vec<String> },
    Wizard(String),
    WizardDone,
    Back,
    Exit,
    SameMonitor,
    ShareOn,
    ShareOff,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PageKind {
    #[default]
    Main,
    Play,
    Recomp,
    Retail,
    Tools,
    OptIns,
    Settings,
    Versions,
    Label,
    Clear,
    Share,
    SetupCheck,
    Wizard,
    Text,
}

#[derive(Clone, Default)]
struct Page {
    kind: PageKind,
    title: String,
    items: Vec<Item>,
    sel: usize,
    body: Vec<String>,
    mono: Option<String>,
}

/// A running job's state, filled by the reader thread from the child's output (doc 44a 1e).
#[derive(Default)]
struct JobState {
    lines: Vec<String>,
    building: Option<String>,
    progress: Option<(u32, u32)>,
    krate: String,
    compiled: u32,
    warnings: u32,
    first_error: Vec<String>,
    waiting: Option<String>,
    stage: String,
    done: Option<i32>,
    commit: String,
    ended: Option<bool>,
    log_path: Option<String>,
    log: Option<std::fs::File>,
    active: bool,
    build_start: Option<Instant>,
    build_end: Option<Instant>,
    /// A fetch the backend waits on (LAUNCHER_WAIT begin): what and since when.
    prewait: Option<(String, Instant)>,
    /// Who holds the build folder (LAUNCHER_BUILD wait) and since when the build waits.
    wait_who: Option<String>,
    wait_since: Option<Instant>,
}

impl JobState {
    /// `BuildView.FetchWaiting`: a fetch that has taken 5 s or more.
    fn fetch_waiting(&self) -> bool {
        self.prewait.as_ref().is_some_and(|(_, since)| since.elapsed() >= Duration::from_secs(5))
    }
    fn end_wait(&mut self) {
        self.waiting = None;
        self.wait_since = None;
    }
}

struct Job {
    title: String,
    state: Arc<Mutex<JobState>>,
    child: Arc<Mutex<Option<std::process::Child>>>,
    started: Instant,
    cancel_presses: u8,
    close_after: Option<Duration>,
    cancelled: Option<String>,
}

pub struct App {
    exe: PathBuf,
    state_dir: PathBuf,
    pages: Vec<Page>,
    job: Option<Job>,
    status: Vec<(String, String)>,
    gilrs: Option<gilrs::Gilrs>,
    last_nav: Instant,
    hidden: bool,
    wizard: Vec<(String, String)>,
    /// The wizard's values when it opened (only changed keys are saved).
    wizard_start: Vec<(String, String)>,
    direct_close: Option<Instant>,
    opened: Instant,
    snapshot: Option<PathBuf>,
    snapshot_asked: bool,
    hover: Option<crate::camcorder::Key>,
    wear: Option<egui::TextureHandle>,
    snapshot_seed: i32,
    /// Off Windows: size the window from its monitor on the first frame (window.rs).
    fit_pending: bool,
    raise_checked: Instant,
    anim: crate::camcorder::Anim,
    /// Begin asked for while a game runs: started when it closes (the Waiting page).
    waiting_for: Option<(String, String)>,
    games_checked: Instant,
    games_running: bool,
    pad_prev: u8,
    pad_since: Instant,
    start: Option<StartCheck>,
    direct: Vec<String>,
    pub after_update: bool,
    pub same_monitor: bool,
    /// The updated launcher started by restart_if_updated: this window waits hidden until it ends.
    restarted: Option<std::process::Child>,
    started: bool,
    cache: Cache,
    relist_pending: bool,
    snapshot_screen: String,
    snapshot_tries: u32,
    wear_ms: f32,
    wear_rx: Option<std::sync::mpsc::Receiver<(tiny_skia::Pixmap, f32)>>,
    /// `--bench <out.txt>`: the render's CPU time at five points of the power-on, 20 frames each (SkateLauncher.cs Bench).
    pub bench: Option<(PathBuf, usize, Vec<f32>, String)>,
    last_act: (u8, bool, Instant),
    powered_off: bool,
    /// A game's window took over: the panel is folding shut, then the window minimises.
    folding: bool,
}

/// The start check (`pending`: self-update, assets links, the last session's check) behind the splash, off the UI
/// thread (SkateLauncher.cs Start / RunPending / SkipPress / FinishStart).
struct StartCheck {
    since: Instant,
    stage: Arc<Mutex<String>>,
    fetching: Arc<std::sync::atomic::AtomicBool>,
    fetched: Arc<std::sync::atomic::AtomicBool>,
    skipped: Arc<std::sync::atomic::AtomicBool>,
    child: Arc<Mutex<Option<std::process::Child>>>,
    result: Arc<Mutex<Option<i32>>>,
    skip_armed: bool,
}

const SPLASH_MIN: Duration = Duration::from_millis(1100);

impl StartCheck {
    fn spawn(exe: PathBuf, cache: Option<Cache>) -> StartCheck {
        use std::sync::atomic::{AtomicBool, Ordering};
        let s = StartCheck {
            since: Instant::now(), stage: Arc::new(Mutex::new(String::new())), fetching: Arc::new(AtomicBool::new(false)),
            fetched: Arc::new(AtomicBool::new(false)), skipped: Arc::new(AtomicBool::new(false)), child: Arc::new(Mutex::new(None)),
            result: Arc::new(Mutex::new(None)), skip_armed: false,
        };
        let (stage, fetching, fetched, skipped, child, result, since) = (s.stage.clone(), s.fetching.clone(), s.fetched.clone(), s.skipped.clone(), s.child.clone(), s.result.clone(), s.since);
        std::thread::spawn(move || {
            let run = |no_self: bool| -> i32 {
                let mut cmd = crate::proc::cmd(&exe);
                cmd.arg("pending").env("SKATE_LAUNCHER_UI", "2").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
                if no_self { cmd.env("SKATE_LAUNCHER_NO_SELF", "1"); }
                let Ok(mut c) = cmd.spawn() else { return 0 };
                let out = c.stdout.take();
                if let Ok(mut g) = child.lock() { *g = Some(c); }
                if let Some(out) = out {
                    use std::io::BufRead;
                    for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                        if line.starts_with("LAUNCHER_SELF\tfetching") { fetching.store(true, Ordering::SeqCst); }
                        else if line.starts_with("LAUNCHER_SELF\tfetched") { fetched.store(true, Ordering::SeqCst); }
                        else if let Some(st) = line.strip_prefix("LAUNCHER_STAGE\t") { if let Ok(mut g) = stage.lock() { *g = st.to_owned(); } }
                    }
                }
                let code = child.lock().ok().and_then(|mut g| g.take()).and_then(|mut c| c.wait().ok()).and_then(|s| s.code()).unwrap_or(0);
                code
            };
            let mut code = run(false);
            // Skipped during the fetch: run again without the self-update so the rest of the check still happens.
            if skipped.load(Ordering::SeqCst) { code = run(true); }
            // The menu's lists load behind the splash (not in direct mode or when an updated launcher takes over).
            if let Some(cache) = cache {
                if let Ok(mut g) = stage.lock() { *g = "Loading the menu".into(); }
                prefetch_into(&exe, &cache, true);
            }
            if let Some(left) = SPLASH_MIN.checked_sub(since.elapsed()) { std::thread::sleep(left); }
            if let Ok(mut g) = result.lock() { *g = Some(code); }
        });
        s
    }

    fn idle() -> StartCheck {
        use std::sync::atomic::AtomicBool;
        StartCheck {
            since: Instant::now(), stage: Arc::new(Mutex::new(String::new())), fetching: Arc::new(AtomicBool::new(false)),
            fetched: Arc::new(AtomicBool::new(false)), skipped: Arc::new(AtomicBool::new(false)), child: Arc::new(Mutex::new(None)),
            result: Arc::new(Mutex::new(None)), skip_armed: false,
        }
    }

    fn can_skip(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.fetching.load(Ordering::SeqCst) && !self.fetched.load(Ordering::SeqCst) && !self.skipped.load(Ordering::SeqCst) && self.since.elapsed() >= Duration::from_secs(3)
    }

    /// B during the self-update fetch: the first press arms, the second ends the check's process.
    fn skip_press(&mut self) {
        if !self.can_skip() { return; }
        if !self.skip_armed { self.skip_armed = true; return; }
        if let Ok(mut g) = self.child.lock() {
            if let Some(c) = g.as_mut() {
                self.skipped.store(true, std::sync::atomic::Ordering::SeqCst);
                kill_tree(c.id());
            }
        }
    }

    fn stage_text(&self) -> String {
        if self.skipped.load(std::sync::atomic::Ordering::SeqCst) { return "Finishing the start".into(); }
        self.stage.lock().ok().map(|s| s.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Checking for a newer launcher".into())
    }
}

/// The lists the window shows, read from the backend in the background so pages open at once (SkateLauncher.cs
/// Prefetched / Listed: `entries` and `status` take most of a second each).
const PREFETCHED: [&str; 4] = ["entries", "versions", "notes", "status"];
type Cache = Arc<Mutex<std::collections::HashMap<String, Vec<String>>>>;

fn prefetch_into(exe: &PathBuf, cache: &Cache, wait: bool) {
    let handles: Vec<_> = PREFETCHED.iter().map(|name| {
        let (exe, cache, name) = (exe.clone(), cache.clone(), (*name).to_owned());
        std::thread::spawn(move || {
            let lines = query(&exe, &[&name]);
            if let Ok(mut c) = cache.lock() { c.insert(name, lines); }
        })
    }).collect();
    if wait { for h in handles { let _ = h.join(); } }
}

/// Runs a backend command and returns its lines (short query commands).
fn query(exe: &PathBuf, args: &[&str]) -> Vec<String> {
    crate::proc::cmd(exe).args(args).env("SKATE_LAUNCHER_UI", "2").stdin(Stdio::null()).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_owned).collect()).unwrap_or_default()
}

fn split(line: &str) -> Vec<String> {
    line.split('\t').map(str::to_owned).collect()
}

/// One line of a job's output into the state (cargo progress, the launcher markers, the first error).
fn feed(s: &mut JobState, line: &str) {
    let f: Vec<&str> = line.split('\t').collect();
    match f.first().copied() {
        Some("LAUNCHER_BUILD") => match f.get(1).copied() {
            Some("begin") => {
                s.building = Some(f.get(2).copied().unwrap_or_default().to_owned());
                s.commit = f.get(3).copied().unwrap_or_default().to_owned();
                s.log_path = f.get(4).map(|p| (*p).to_owned()).filter(|p| !p.is_empty());
                s.active = true;
                s.ended = None;
                s.progress = None;
                s.compiled = 0;
                s.warnings = 0;
                s.first_error.clear();
                s.build_start = Some(Instant::now());
                s.waiting = None;
                // The build log is written here from the stream, as the C# window does (doc 44a 1e).
                s.log = s.log_path.as_ref().and_then(|p| {
                    let p = std::path::Path::new(p);
                    if let Some(d) = p.parent() { let _ = std::fs::create_dir_all(d); }
                    std::fs::File::create(p).ok()
                });
            }
            Some("wait") => {
                s.wait_who = f.get(3).map(|w| (*w).to_owned());
                // "hold": the backend itself waits (the version's game is running), shown as a wait.
                if f.get(4).copied() == Some("hold") {
                    if s.waiting.is_none() { s.wait_since = Some(Instant::now()); }
                    s.waiting = s.wait_who.clone();
                }
            }
            Some("waitend") => s.end_wait(),
            Some("end") => {
                s.ended = Some(f.get(2).copied() == Some("0"));
                s.active = false;
                s.build_end = Some(Instant::now());
                s.log = None;
            }
            _ => {}
        },
        Some("LAUNCHER_WAIT") => match f.get(1).copied() {
            Some("begin") => {
                s.stage = format!("Fetching {}", f.get(3).copied().unwrap_or_default());
                s.prewait = Some((format!("fetching {} from {}", f.get(3).copied().unwrap_or_default(), f.get(4).copied().unwrap_or_default()), Instant::now()));
            }
            _ => {
                s.stage.clear();
                s.prewait = None;
            }
        },
        Some("LAUNCHER_STAGE") => s.stage = f.get(1).copied().unwrap_or_default().to_owned(),
        Some("LAUNCHER_SELF") => {}
        _ => {
            let cleaned = clean(line);
            let t = cleaned.trim_start();
            if s.active {
                if let Some(f) = s.log.as_mut() {
                    // Progress-bar redraws stay out of the log (BuildView.Feed barOnly).
                    if !t.is_empty() && !(t.starts_with("Building [") && !t.contains("Compiling")) {
                        use std::io::Write;
                        let _ = writeln!(f, "{cleaned}");
                    }
                }
            }
            if let Some(rest) = t.strip_prefix("Building [") {
                if let Some((_, after)) = rest.split_once("] ") {
                    let (count, krate) = after.split_once(": ").unwrap_or((after, ""));
                    if let Some((a, b)) = count.split_once('/') {
                        if let (Ok(a), Ok(b)) = (a.trim().parse(), b.trim().parse()) {
                            s.progress = Some((a, b));
                        }
                    }
                    s.krate = krate.to_owned();
                }
                return;
            }
            if t.starts_with("Compiling ") {
                s.compiled += 1;
                s.krate = t.trim_start_matches("Compiling ").to_owned();
            } else if t.starts_with("warning: ") && !t.contains("generated") {
                s.warnings += 1;
            } else if t.starts_with("error") && s.first_error.is_empty() {
                s.first_error.push(t.to_owned());
            } else if !s.first_error.is_empty() && s.first_error.len() < 40 {
                s.first_error.push(cleaned.clone());
            }
            if let Some(what) = t.strip_prefix("Blocking waiting for file lock on ") {
                if s.waiting.is_none() { s.wait_since = Some(Instant::now()); }
                s.waiting = Some(format!("another build is using the {}", what.trim()));
            } else if s.active && (t.starts_with("Building [") || t.starts_with("Compiling ")) {
                s.end_wait();
            }
            s.lines.push(cleaned.clone());
            if s.lines.len() > 400 {
                s.lines.remove(0);
            }
        }
    }
}

impl App {
    pub fn new(exe: PathBuf, state_dir: PathBuf, direct: Vec<String>) -> App {
        let mut app = App {
            exe, state_dir, pages: Vec::new(), job: None, status: Vec::new(), gilrs: gilrs::Gilrs::new().ok(),
            last_nav: Instant::now(), hidden: false, wizard: Vec::new(), wizard_start: Vec::new(), direct_close: None, opened: Instant::now(), snapshot: None, snapshot_asked: false, hover: None, wear: None, snapshot_seed: 3, fit_pending: false, raise_checked: Instant::now(), anim: crate::camcorder::Anim::new(crate::wear::launch_seed()), powered_off: false, folding: false, waiting_for: None, games_checked: Instant::now(), games_running: false, pad_prev: 0, pad_since: Instant::now(), start: None, direct: Vec::new(), after_update: false, same_monitor: false, restarted: None, started: false, cache: Arc::new(Mutex::new(std::collections::HashMap::new())), relist_pending: false, snapshot_screen: String::new(), snapshot_tries: 0, wear_ms: 0.0, wear_rx: None, bench: None, last_act: (0, false, Instant::now()),
        };
        app.direct = direct;
        app
    }

    /// The window shows first; the start check runs behind the splash (snapshots skip it and show the menu at once).
    fn begin_start(&mut self) {
        if self.bench.is_some() {
            self.finish_start(0);
        } else if self.snapshot.is_some() {
            self.finish_start(0);
            self.snapshot_screen_setup();
        } else {
            let cache = (self.direct.is_empty() && !self.state_dir.join("restart.txt").exists()).then(|| self.cache.clone());
            self.start = Some(StartCheck::spawn(self.exe.clone(), cache));
        }
    }

    fn finish_start(&mut self, code: i32) {
        self.start = None;
        if self.restart_if_updated(code == 10) { return; }
        if !self.direct.is_empty() {
            let d = std::mem::take(&mut self.direct);
            self.start_direct(d);
            return;
        }
        self.refresh_status();
        self.pages.push(self.main_page());
        if self.status_of("setup_missing").is_some_and(|m| !m.is_empty()) {
            let p = self.setup_page();
            self.pages.push(p);
        }
        if self.status_of("first_setup").is_some_and(|f| f == "1") {
            let p = self.wizard_page();
            self.pages.push(p);
        }
        if code == 10 || self.after_update {
            let text = self.last_session();
            let title = if self.after_update || text.iter().any(|l| l.contains("launcher update")) { "Launcher updated" } else { "Previous session (closed before its check)" };
            self.pages.push(text_page(title, text, None));
        }
    }

    /// The launcher replaced its own program (the self-update wrote restart.txt): start the new one and wait for it
    /// hidden, so Steam keeps the process it started (its controller layout and "playing" state) until the new window
    /// closes (SkateLauncher.cs RestartIfUpdated).
    fn restart_if_updated(&mut self, show_last: bool) -> bool {
        let f = self.state_dir.join("restart.txt");
        if !f.exists() { return false; }
        let _ = std::fs::remove_file(&f);
        let Ok(me) = std::env::current_exe() else { return false };
        // The window's program in its folder (an update or switch renamed the running one aside).
        let named = me.parent().map(|d| d.join(if cfg!(windows) { "SkateLauncher.exe" } else { "skate-launcher" }));
        let exe = named.filter(|p| p.exists()).unwrap_or(me);
        let mut args = self.direct.clone();
        if show_last { args.push("--after-update".into()); }
        if self.same_monitor { args.push("--same-monitor".into()); }
        crate::window::release_instance();
        let Ok(child) = crate::proc::cmd(&exe).args(&args).spawn() else { return false };
        self.restarted = Some(child);
        true
    }

    fn start_direct(&mut self, args: Vec<String>) {
        let mut cmd = args.clone();
        if let Some(at) = cmd.iter().position(|a| a.starts_with('@')) {
            let v = cmd.remove(at);
            cmd.push("-Version".into());
            cmd.push(v.trim_start_matches('@').into());
        }
        let first = cmd[0].clone();
        if first.starts_with("version-") || first == "notes" {
            let lines = query(&self.exe, &cmd.iter().map(String::as_str).collect::<Vec<_>>());
            self.pages.push(text_page(&first, lines, None));
            self.direct_close = Some(Instant::now() + Duration::from_secs(15));
            return;
        }
        self.pages.push(text_page("Starting", vec![first.clone()], None));
        self.run_job(&first, cmd);
        if let Some(j) = &mut self.job {
            j.close_after = Some(Duration::from_secs(10));
        }
    }

    /// `--snapshot-page`: the page the snapshot shows (main, play, tools), on top of the start pages' main screen.
    fn snapshot_page(&mut self, page: Option<&str>) {
        let Some(page) = page else { return };
        let mut screen = page.to_owned();
        if let Some((s, seed)) = screen.clone().split_once('#') {
            if let Ok(n) = seed.parse() { self.snapshot_seed = n; }
            screen = s.to_owned();
        }
        if let Some((s, at)) = screen.clone().split_once('@') {
            self.anim.snap = at.parse().ok();
            screen = s.to_owned();
        } else {
            self.anim.snap = Some(1.0);
        }
        self.snapshot_screen = screen;
    }

    fn bench_frame(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        const AT: [f32; 5] = [0.3, 0.5, 0.65, 0.9, 1.0];
        let Some((out, step, times, text)) = self.bench.as_mut() else { return };
        if *step < AT.len() {
            self.anim.snap = Some(AT[*step]);
            // The first frames of each step settle; the CPU time of the next 20 counts.
            if let Some(cpu) = frame.info().cpu_usage { times.push(cpu * 1000.0); }
            if times.len() >= 24 {
                let ms: f32 = times[4..].iter().sum::<f32>() / 20.0;
                text.push_str(&format!("assembly {:.2}: render {ms:.1} ms\n", AT[*step]));
                times.clear();
                *step += 1;
            }
            ctx.request_repaint();
        } else if self.wear.is_some() {
            text.push_str(&format!("wear texture: {:.1} ms (built off the window's thread)\n", self.wear_ms));
            let _ = std::fs::write(&*out, text.as_bytes());
            print!("{text}");
            self.bench = None;
            self.powered_off = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint();
        }
    }

    /// Sets up the snapshot's screen once the main page is there.
    fn snapshot_screen_setup(&mut self) {
        let screen = std::mem::take(&mut self.snapshot_screen);
        let fake_job = |title: &str, lines: &[&str], build_secs: Option<u64>, started_secs: u64| {
            let mut s = JobState::default();
            for l in lines { feed(&mut s, l); }
            if let Some(b) = build_secs { s.build_start = Instant::now().checked_sub(Duration::from_secs(b)); }
            if s.waiting.is_some() { s.wait_since = Instant::now().checked_sub(Duration::from_secs(72)); }
            if let Some((_, since)) = s.prewait.as_mut() { *since = Instant::now().checked_sub(Duration::from_secs(42)).unwrap_or(*since); }
            Job { title: title.into(), state: Arc::new(Mutex::new(s)), child: Arc::new(Mutex::new(None)),
                started: Instant::now().checked_sub(Duration::from_secs(started_secs)).unwrap_or_else(Instant::now), cancel_presses: 0, close_after: None, cancelled: None }
        };
        match screen.as_str() {
            "checking" => {
                self.pages.clear();
                let s = StartCheck::idle();
                s.fetching.store(true, std::sync::atomic::Ordering::SeqCst);
                self.start = Some(StartCheck { since: Instant::now().checked_sub(Duration::from_secs(7)).unwrap_or_else(Instant::now), ..s });
            }
            "versions" => { let p = self.versions_page(); self.pages.push(p); }
            "tools" => self.open(PageKind::Tools),
            "setup" => self.open(PageKind::Wizard),
            "play" | "play-traceall" => {
                self.open(PageKind::Play);
                if screen == "play-traceall" {
                    if let Some(p) = self.pages.last_mut() { p.sel = 1.min(p.items.len().saturating_sub(1)); }
                }
            }
            "waiting" => self.job = Some(fake_job("Update and build", &[
                "LAUNCHER_BUILD\tbegin\thails-additions\t94774ce\t",
                "LAUNCHER_BUILD\twait\tbuild\thails-additions is being built by the shell since 16:41 (pid 21412)",
                "    Blocking waiting for file lock on build directory",
            ], Some(75), 75)),
            "fetching" => self.job = Some(fake_job("Update and build", &[
                "  Checking hails-additions on origin...",
                "LAUNCHER_WAIT\tbegin\tfetch\thails-additions\torigin",
            ], None, 44)),
            "build" | "failed" => {
                let mut lines = vec!["LAUNCHER_BUILD\tbegin\thails-additions\t94774ce\t", "   Compiling skate-core v0.1.0", "warning: unused import: `skitch_enabled`",
                    " --> crates\\skate-game\\src\\physics.rs:81:53", "", "    Building [=======>        ] 312/540: skate-game, bevy_pbr"];
                if screen == "failed" {
                    lines.extend(["error[E0308]: mismatched types", "  --> crates\\skate-game\\src\\physics\\grab_view.rs:42:5", "   |",
                        "42 |     let x: u32 = 1.0;", "   |            ---   ^^^ expected `u32`, found floating-point number", ""]);
                }
                self.job = Some(fake_job("Update and build", &lines, Some(190), 190));
            }
            "job" => self.job = Some(fake_job("Building hails-additions", &["Fetching hails-additions at 94774ce", "   Compiling bevy_render v0.18.1",
                "   Compiling skate-core v0.1.0", "warning: unused import: `skitch_enabled`", "   Compiling skate-game v0.1.0"], None, 74)),
            _ => {}
        }
    }

    fn refresh_status(&mut self) {
        self.status = self.listed("status").iter().filter_map(|l| l.split_once('\t').map(|(k, v)| (k.to_owned(), v.to_owned()))).collect();
    }

    /// A list from the cache, else read now (and kept).
    fn listed(&self, name: &str) -> Vec<String> {
        if let Some(l) = self.cache.lock().ok().and_then(|c| c.get(name).cloned()) { return l; }
        let lines = query(&self.exe, &[name]);
        if let Ok(mut c) = self.cache.lock() { c.insert(name.to_owned(), lines.clone()); }
        lines
    }

    /// `RebuildMain`: a version change or a finished job can change every list: read them again in the background and
    /// rebuild the menu when they are in.
    fn relist(&mut self) {
        if let Ok(mut c) = self.cache.lock() { for n in PREFETCHED { c.remove(n); } }
        prefetch_into(&self.exe, &self.cache, false);
        self.relist_pending = true;
    }

    fn status_of(&self, k: &str) -> Option<&str> {
        self.status.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
    }

    fn last_session(&self) -> Vec<String> {
        std::fs::read_to_string(self.state_dir.join("last_session.txt")).map(|t| t.trim_start_matches('\u{feff}').lines().map(str::to_owned).collect())
            .unwrap_or_else(|_| vec!["No session yet.".into()])
    }

    fn entries(&self) -> Vec<Vec<String>> {
        self.listed("entries").iter().map(|l| split(l)).filter(|f| f.len() >= 9).collect()
    }

    fn main_page(&self) -> Page {
        let entries = self.entries();
        let has = |g: &str| entries.iter().any(|e| e[4] == g);
        let item = |text: &str, hint: &str, on: bool, action: Action| Item { text: text.into(), hint: hint.into(), on, action, ..Default::default() };
        let version = self.status_of("version").unwrap_or_default().to_owned();
        let launcher = self.status_of("launcher").unwrap_or_default();
        let newer = self.status_of("launcher_newer").unwrap_or_default();
        let _ = (launcher, newer);
        let title = "Skate launcher".to_owned();
        Page {
            kind: PageKind::Main, title,
            items: vec![
                item(&format!("Play: {version}"), &format!("Our engine, the selected version ({}).", self.status_of("version_status").unwrap_or_default()), true, Action::Open(PageKind::Play)),
                item("Versions", "Pick the build Play starts.", true, Action::Open(PageKind::Versions)),
                item("What to test", "What changed and what to try.", true, Action::Job(vec!["notes".into()])),
                item("Recomp", if has("recomp") { "Recompiled retail, traced or not." } else { "Not set up (see README.md)." }, has("recomp"), Action::Open(PageKind::Recomp)),
                item("RPCS3", if has("retail") { "The PS3 game in RPCS3." } else { "Not set up (see README.md)." }, has("retail"), Action::Open(PageKind::Retail)),
                item("Tools", "Game files, builds, logs, recomp, setup.", true, Action::Open(PageKind::Tools)),
                item("Last session", "What was checked and sent.", true, Action::Open(PageKind::Text)),
                item("Exit", "Close the launcher.", true, Action::Exit),
            ],
            ..Default::default()
        }
    }

    fn entry_page(&self, kind: PageKind, group: &str, title: &str) -> Page {
        let mut items: Vec<Item> = self.entries().into_iter().filter(|e| e[4] == group).map(|e| Item {
            text: e[1].clone(), hint: if e[5] == "1" { e[2].clone() } else { e[6].clone() }, on: e[5] == "1",
            info: e[7].clone(), records: e[8].split('|').filter(|r| !r.is_empty()).map(str::to_owned).collect(),
            action: Action::Begin { entry: e[0].clone(), ask: e[3] == "1" },
        }).collect();
        if group == "rust" || group == "tools" {
            items.push(self.share_item());
        }
        if kind == PageKind::Tools {
            let more = [
                ("Opt-ins and settings", "The game's opt-ins and settings, the same ones as its Guide / F1 overlay.", Action::Open(PageKind::OptIns)),
            ];
            items.extend(more.into_iter().map(|(t, h, a)| Item { text: t.into(), hint: h.into(), on: true, action: a, ..Default::default() }));
            let same = crate::window::same_monitor_file(&self.state_dir).exists();
            items.push(Item {
                text: format!("Open on: {}", if same { "the monitor it was started from" } else { "the main monitor" }),
                hint: "Where the launcher window opens. --same-monitor on the command line does the same for one start.".into(),
                on: true, action: Action::SameMonitor, ..Default::default()
            });
            let more = [
                ("Set up the launcher", "Game folder, space for builds, the Skate 3 disc, the recomp and RPCS3.", Action::Open(PageKind::Wizard)),
                ("Clear builds and build caches", "Frees disk space: everything, or one version's build cache or whole build. Asks before deleting.", Action::Open(PageKind::Clear)),
            ];
            items.extend(more.into_iter().map(|(t, h, a)| Item { text: t.into(), hint: h.into(), on: true, action: a, ..Default::default() }));
        }
        if items.is_empty() {
            items.push(Item { text: "Nothing here yet".into(), hint: "See README.md to set these up.".into(), on: false, ..Default::default() });
        }
        Page { kind, title: title.into(), items, ..Default::default() }
    }

    fn settings_page(&self, kind: PageKind) -> Page {
        let group = if kind == PageKind::OptIns { "Opt-ins" } else { "Settings" };
        let mut items: Vec<Item> = query(&self.exe, &["game-settings"]).iter().map(|l| split(l)).filter(|f| f.len() >= 5 && f[1] == group)
            .map(|f| Item { text: format!("{}: {}", f[2], f[3]), hint: f[4].clone(), on: true, action: Action::Setting(f[0].clone()), ..Default::default() }).collect();
        if kind == PageKind::OptIns {
            items.push(Item { text: "Settings".into(), hint: "Resolution, frame rate, volumes, difficulty.".into(), on: true, action: Action::Open(PageKind::Settings), ..Default::default() });
        }
        Page { kind, title: if kind == PageKind::OptIns { "Opt-ins".into() } else { "Settings".into() }, items, ..Default::default() }
    }

    fn versions_page(&self) -> Page {
        let rows: Vec<Vec<String>> = self.listed("versions").iter().map(|l| split(l)).filter(|f| f.len() >= 4).collect();
        let sel = rows.iter().position(|f| f[3] == "*").unwrap_or(0);
        let items = rows.into_iter().map(|f| Item {
            text: format!("{}{}", if f[3] == "*" { "> " } else { "" }, f[1]), hint: f[2].clone(), on: true, action: Action::SetVersion(f[0].clone()), ..Default::default()
        }).collect();
        Page { kind: PageKind::Versions, title: "Choose version".into(), items, sel, ..Default::default() }
    }

    fn clear_page(&self) -> Page {
        let gb = |b: &str| b.parse::<f64>().unwrap_or(0.0) / 1073741824.0;
        let mut items = vec![
            Item { text: "Everything".into(), hint: "Every version's build cache and build.".into(), on: true,
                action: Action::Confirm { text: "Delete every build and build cache?".into(), args: vec!["clear-build".into(), "all".into()] }, ..Default::default() },
            Item { text: "Every build cache".into(), hint: "The versions still play; the next build takes longer.".into(), on: true,
                action: Action::Confirm { text: "Delete every build cache?".into(), args: vec!["clear-cache".into(), "all".into()] }, ..Default::default() },
        ];
        for f in query(&self.exe, &["sizes"]).iter().map(|l| split(l)).filter(|f| f.len() >= 4) {
            items.push(Item { text: format!("{}: build cache ({:.1} GB)", f[1], gb(&f[2])), hint: "It still plays.".into(), on: true,
                action: Action::Confirm { text: format!("Delete the build cache of {}?", f[1]), args: vec!["clear-cache".into(), f[0].clone()] }, ..Default::default() });
            items.push(Item { text: format!("{}: whole build ({:.1} GB)", f[1], gb(&f[2]) + gb(&f[3])), hint: "It builds again at its next launch.".into(), on: true,
                action: Action::Confirm { text: format!("Delete the whole build of {}?", f[1]), args: vec!["clear-build".into(), f[0].clone()] }, ..Default::default() });
        }
        Page { kind: PageKind::Clear, title: "Clear builds".into(), items, ..Default::default() }
    }

    fn share_page(&self) -> Page {
        let item = |t: &str, h: &str, on: bool, a: Action| Item { text: t.into(), hint: h.into(), on, action: a, ..Default::default() };
        Page { kind: PageKind::Share, title: "Share sessions via GitHub".into(), items: vec![
            item("Turn it on", "After each session the launcher zips the new logs and opens a GitHub issue on the repository of the version you play, in your browser. You drag the zip in and press Submit; nothing is sent without that.", true, Action::ShareOn),
            item("Not now", "Sharing stays off.", true, Action::Back),
            item("What is shared", "Game logs, trace folders and launcher build logs of this PC, with your Windows user name, home folder and PC name replaced. Not shared: saves, game files, recomp sound captures. Issues on public repositories are public. Turn it off here or in Tools at any time.", false, Action::None),
        ], ..Default::default() }
    }

    /// The share toggle row (`ShowShare`).
    fn share_item(&self) -> Item {
        let s = self.status_of("share").unwrap_or_default().to_owned();
        let on = s.starts_with("on ");
        Item {
            text: if s.is_empty() { "Share sessions via GitHub".into() } else { format!("Share sessions: {}", if on { "ON" } else { "OFF" }) },
            hint: if s.is_empty() { "Not in this launcher version.".into() } else if on { format!("After each session the logs are packed and a GitHub issue opens on {}. Press to turn off.", &s[3..]) }
                else if s == "no repo" { "This version has no GitHub repository to send to.".into() } else { "Off. Press to read what is shared and turn it on.".into() },
            on: !s.is_empty() && s != "no repo",
            action: if on { Action::ShareOff } else { Action::Open(PageKind::Share) },
            ..Default::default()
        }
    }

    /// The page under the top one rebuilt (after a toggle changed what it shows), keeping its selection.
    fn rebuild_entry_page(&mut self, depth_from_top: usize) {
        let n = self.pages.len();
        if n <= depth_from_top { return; }
        let i = n - 1 - depth_from_top;
        let (kind, sel) = (self.pages[i].kind, self.pages[i].sel);
        let mut page = match kind {
            PageKind::Play => self.entry_page(kind, "rust", &format!("Play: {}", self.status_of("version").unwrap_or_default())),
            PageKind::Tools => self.entry_page(kind, "tools", "Tools"),
            _ => return,
        };
        page.sel = sel.min(page.items.len().saturating_sub(1));
        self.pages[i] = page;
    }

    fn setup_page(&self) -> Page {
        let rows: Vec<Vec<String>> = query(&self.exe, &["setup-check"]).iter().map(|l| split(l)).filter(|f| f.len() >= 5).collect();
        let mut body = vec!["This PC needs a few tools before it can build and play versions:".to_owned(), String::new()];
        body.extend(rows.iter().map(|f| format!("{}{}{}", if f[2] == "1" { "  ok       " } else { "  MISSING  " }, f[1], if f[2] == "1" || f[4] == "1" { "" } else { " (optional)" })));
        let install = if cfg!(windows) { "Windows asks once for permission (UAC). The C++ build tools take 10 minutes or more." } else { "Your system asks once for your password; the package manager installs the rest." };
        Page { kind: PageKind::SetupCheck, title: "First-time setup".into(), body, items: vec![
            Item { text: "Install what is missing".into(), hint: install.into(), on: true, action: Action::BeginLabel { entry: "tool-setup".into(), label: String::new() }, ..Default::default() },
            Item { text: "Not now".into(), hint: "The Play entries stay greyed out until the tools are there.".into(), on: true, action: Action::Back, ..Default::default() },
        ], ..Default::default() }
    }

    /// Tools > Set up the launcher (SkateLauncher.cs WizardPage): the game folder, the space for builds, the disc, the
    /// recomp and RPCS3; "config-get" reads them, Done saves the changed ones through "config-set".
    fn wizard_page(&mut self) -> Page {
        if self.wizard.is_empty() {
            self.wizard = query(&self.exe, &["config-get"]).iter().filter_map(|l| l.split_once('\t').map(|(k, v)| (k.to_owned(), v.to_owned()))).collect();
            let get = |k: &str| self.wizard.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap_or_default();
            if get("storage_root").is_empty() {
                let root = if get("target_root").is_empty() { get("default_root") } else { get("target_root") };
                self.wizard_set("storage_root", root);
            }
            self.wizard_start = self.wizard.clone();
        }
        let get = |k: &str| self.wizard.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap_or_default();
        let now = |v: String, unset: &str| format!("Now: {}\n\n", if v.is_empty() { unset.to_owned() } else { v });
        let root = get("storage_root");
        let free = free_on(&root);
        let cap: i64 = get("build_cap_gb").parse().unwrap_or(0);
        let item = |t: &str, h: String, a: Action| Item { text: t.into(), hint: h, on: true, action: a, ..Default::default() };
        let keeps = if get("standalone") == "1" { "Where the launcher keeps the game versions and their builds" } else { "Where the launcher keeps the builds" };
        Page { kind: PageKind::Wizard, title: "Set up the launcher".into(), items: vec![
            item("Game folder", format!("{}{keeps} (one version with its build takes about 15 to 45 GB). {free}", now(root.clone(), "not set")), Action::Wizard("storage_root".into())),
            item("Space for builds", format!("{}Press to change. Before a build would go over it, the launcher deletes the build cache of the version built longest ago. {free}",
                now(if cap > 0 { format!("{cap} GB") } else { "no limit".into() }, "")), Action::Wizard("build_cap_gb".into())),
            item("Skate 3 disc", format!("{}Your Xbox 360 Skate 3 ISO or default.xex. The first play converts the game files from it once.", now(get("disc"), "not set")), Action::Wizard("disc".into())),
            item("Skate 3 Recompiled", format!("{}The skate3recomp game folder (with skate3.exe). Adds the Recomp entries.", now(get("recomp.working_dir"), "not set (optional)")), Action::Wizard("recomp".into())),
            item("RPCS3", format!("{}rpcs3.exe, to play retail Skate 3 (PS3) for comparison.", now(get("rpcs3.exe"), "not set (optional)")), Action::Wizard("rpcs3.exe".into())),
            item("RPCS3 Skate 3 game", format!("{}The folder of your Skate 3 PS3 game (the one with PS3_GAME or EBOOT.BIN).", now(get("rpcs3.game"), "not set (optional)")), Action::Wizard("rpcs3.game".into())),
            item("Done", "Saves these settings. Change them later in Tools > Set up the launcher.".into(), Action::WizardDone),
            item("Not now", "Saves nothing; this page opens again at the next start.".into(), Action::Back),
        ], ..Default::default() }
    }

    fn wizard_set(&mut self, key: &str, value: String) {
        match self.wizard.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => self.wizard.push((key.into(), value)),
        }
    }

    fn wizard_pick(&mut self, key: &str) {
        match key {
            "storage_root" => if let Some(p) = crate::pick::folder("The game folder (versions and their builds)") {
                self.wizard_set("storage_root", p.display().to_string());
            },
            "build_cap_gb" => {
                let steps = [0, 50, 100, 150, 200, 300, 500];
                let cur: i64 = self.wizard.iter().find(|(k, _)| k == "build_cap_gb").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
                let next = steps.iter().position(|s| *s == cur).map_or(steps[1], |i| steps[(i + 1) % steps.len()]);
                self.wizard_set("build_cap_gb", next.to_string());
            }
            "disc" => if let Some(p) = crate::pick::file("Your Skate 3 ISO or default.xex", "Skate 3 disc|*.iso;default.xex|All files|*.*") { self.wizard_set("disc", p.display().to_string()) },
            "recomp" => if let Some(p) = crate::pick::folder("The Skate 3 Recompiled folder") {
                let exe = Some(p.join("skate3.exe")).filter(|x| x.exists()).or_else(|| find_file(&p, "skate3.exe", 6))
                    .or_else(|| crate::pick::file("Choose the recomp's skate3.exe", "skate3.exe|skate3.exe|Programs|*.exe"));
                if let Some(e) = exe {
                    self.wizard_set("recomp.working_dir", p.display().to_string());
                    self.wizard_set("recomp.exe", e.display().to_string());
                }
            },
            "rpcs3.exe" => if let Some(p) = crate::pick::file("RPCS3", "RPCS3|rpcs3.exe;rpcs3;*.AppImage|All files|*.*") { self.wizard_set("rpcs3.exe", p.display().to_string()) },
            "rpcs3.game" => if let Some(p) = crate::pick::folder("The Skate 3 game folder for RPCS3") { self.wizard_set("rpcs3.game", p.display().to_string()) },
            _ => {}
        }
        let at = self.pages.last().map_or(0, |p| p.sel);
        let mut page = self.wizard_page();
        page.sel = at;
        *self.pages.last_mut().expect("page") = page;
    }

    /// `SaveWizard`: the changed values only; a new game folder also sets the builds folder, the cap folder and (for a
    /// standalone launcher) the versions folder.
    fn wizard_done(&mut self) {
        let get = |v: &[(String, String)], k: &str| v.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap_or_default();
        let (now, start) = (self.wizard.clone(), self.wizard_start.clone());
        let mut changed = serde_json::Map::new();
        for k in ["disc", "recomp.exe", "recomp.working_dir", "rpcs3.exe", "rpcs3.game"] {
            if get(&now, k) != get(&start, k) { changed.insert(k.into(), serde_json::json!(get(&now, k))); }
        }
        if get(&now, "build_cap_gb") != get(&start, "build_cap_gb") {
            changed.insert("build_cap_gb".into(), serde_json::json!(get(&now, "build_cap_gb").parse::<i64>().unwrap_or(0)));
        }
        let root = get(&now, "storage_root");
        if root != get(&start, "storage_root") && !root.is_empty() {
            let sub = |s: &str| std::path::Path::new(&root).join(s).display().to_string();
            changed.insert("storage_root".into(), serde_json::json!(root));
            changed.insert("target_root".into(), serde_json::json!(sub("builds")));
            changed.insert("build_cap_dir".into(), serde_json::json!(root));
            if get(&now, "standalone") == "1" { changed.insert("versions_root".into(), serde_json::json!(sub("versions"))); }
        }
        changed.insert("first_setup_done".into(), serde_json::json!(true));
        let file = self.state_dir.join("config-set.json");
        let _ = std::fs::write(&file, serde_json::Value::Object(changed).to_string());
        let r = query(&self.exe, &["config-set", &file.to_string_lossy()]).join("\n");
        self.wizard.clear();
        self.pages.pop();
        if r.trim() != "saved" {
            self.pages.push(text_page("Set up the launcher", vec!["The settings could not be saved:".into(), String::new(), r], None));
        }
        self.refresh_status();
        self.rebuild_main();
    }

    fn rebuild_main(&mut self) {
        let main = self.main_page();
        if let Some(first) = self.pages.first_mut() {
            let sel = first.sel;
            *first = main;
            first.sel = sel;
        }
    }

    fn open(&mut self, kind: PageKind) {
        let page = match kind {
            PageKind::Play => self.entry_page(kind, "rust", &format!("Play: {}", self.status_of("version").unwrap_or_default())),
            PageKind::Recomp => self.entry_page(kind, "recomp", "Recomp"),
            PageKind::Retail => self.entry_page(kind, "retail", "Retail Skate 3 (RPCS3)"),
            PageKind::Tools => self.entry_page(kind, "tools", "Tools"),
            PageKind::OptIns | PageKind::Settings => self.settings_page(kind),
            PageKind::Versions => self.versions_page(),
            PageKind::Clear => self.clear_page(),
            PageKind::Share => self.share_page(),
            PageKind::SetupCheck => self.setup_page(),
            PageKind::Wizard => self.wizard_page(),
            PageKind::Text => text_page("Last session", self.last_session(), None),
            PageKind::Main | PageKind::Label => return,
        };
        self.pages.push(page);
    }

    fn run_job(&mut self, title: &str, args: Vec<String>) {
        let state = Arc::new(Mutex::new(JobState::default()));
        let child = crate::proc::cmd(&self.exe).args(&args).env("SKATE_LAUNCHER_UI", "2").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let Ok(mut child) = child else {
            self.pages.push(text_page("Could not start", vec![format!("Could not start {}", self.exe.display())], None));
            return;
        };
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
            let st = state.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                for line in std::io::BufReader::new(stream).lines().map_while(Result::ok) {
                    for part in line.split('\r').filter(|p| !p.trim().is_empty()) {
                        if let Ok(mut s) = st.lock() {
                            feed(&mut s, part);
                        }
                    }
                }
            });
        }
        let child = Arc::new(Mutex::new(Some(child)));
        let (c2, st2) = (child.clone(), state.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(200));
            let code = c2.lock().ok().and_then(|mut g| g.as_mut().and_then(|c| c.try_wait().ok().flatten()).map(|s| s.code().unwrap_or(1)));
            if let Some(code) = code {
                if let Ok(mut s) = st2.lock() {
                    s.done = Some(code);
                }
                return;
            }
        });
        self.job = Some(Job { title: title.into(), state, child, started: Instant::now(), cancel_presses: 0, close_after: None, cancelled: None });
    }

    fn begin(&mut self, entry: &str, label: &str) {
        let games = crate::play::running_games();
        if !games.is_empty() && !entry.starts_with("tool-") {
            self.waiting_for = Some((entry.to_owned(), label.to_owned()));
            self.pages.push(text_page("Waiting", vec![
                format!("Already running: {}", games.join(", ")),
                "Only one game at a time. Close it, or wait here: this continues by itself.".into(),
                String::new(),
                "B: cancel".into(),
            ], None));
            return;
        }
        let name = self.pages.last().and_then(|p| p.items.get(p.sel)).map(|i| i.text.clone()).unwrap_or_else(|| entry.into());
        let mut args = vec![entry.to_owned()];
        if !label.is_empty() {
            args.push(label.to_owned());
        }
        self.run_job(&name, args);
    }

    fn activate(&mut self, ctx: &egui::Context) {
        let Some(item) = self.pages.last().and_then(|p| p.items.get(p.sel)).cloned() else {
            if self.pages.last().is_some_and(|p| p.kind == PageKind::Text) {
                self.back(ctx);
            }
            return;
        };
        if !item.on {
            return;
        }
        match item.action {
            Action::None => {}
            Action::ShareOn => {
                let _ = query(&self.exe, &["share-on"]);
                self.refresh_status();
                self.pages.pop();
                self.rebuild_entry_page(0);
            }
            Action::ShareOff => {
                let _ = query(&self.exe, &["share-off"]);
                self.refresh_status();
                self.rebuild_entry_page(0);
            }
            Action::SameMonitor => {
                let f = crate::window::same_monitor_file(&self.state_dir);
                let _ = if f.exists() { std::fs::remove_file(&f) } else { std::fs::write(&f, "on\n") };
                let sel = self.pages.last().map_or(0, |p| p.sel);
                let mut page = self.entry_page(PageKind::Tools, "tools", "Tools");
                page.sel = sel;
                *self.pages.last_mut().expect("page") = page;
            }
            Action::Open(k) => self.open(k),
            Action::Begin { entry, ask } => {
                if ask {
                    let items = LABELS.iter().map(|l| Item { text: (*l).into(), on: true, action: Action::BeginLabel { entry: entry.clone(), label: (*l).into() }, ..Default::default() }).collect();
                    self.pages.push(Page { kind: PageKind::Label, title: "Session label".into(), items, ..Default::default() });
                } else {
                    self.begin(&entry, "");
                }
            }
            Action::BeginLabel { entry, label } => {
                self.pages.pop();
                self.begin(&entry, &label);
            }
            Action::Job(args) => {
                let lines = if args.len() == 1 && args[0] == "notes" { self.listed("notes") } else { query(&self.exe, &args.iter().map(String::as_str).collect::<Vec<_>>()) };
                if args[0].starts_with("share-") {
                    self.relist();
                    self.pages.pop();
                }
                self.pages.push(text_page(&item.text, lines, None));
            }
            Action::SetVersion(id) => {
                let _ = query(&self.exe, &["version-set", &id]);
                self.pages.pop();
                self.relist();
            }
            Action::Setting(key) => {
                let out = query(&self.exe, &["game-setting", &key]);
                let (kind, sel) = self.pages.last().map(|p| (p.kind, p.sel)).unwrap_or_default();
                let mut page = self.settings_page(kind);
                page.sel = sel;
                *self.pages.last_mut().expect("page") = page;
                if !out.is_empty() {
                    self.pages.push(text_page("Not changed", out, None));
                }
            }
            Action::Confirm { text, args } => {
                let item = |t: &str, a: Action| Item { text: t.into(), on: true, action: a, ..Default::default() };
                self.pages.push(Page { kind: PageKind::Text, title: text, items: vec![item("No, keep it", Action::Back), item("Yes, delete", Action::Job(args.clone()))], ..Default::default() });
            }
            Action::Wizard(key) => self.wizard_pick(&key),
            Action::WizardDone => self.wizard_done(),
            Action::Back => self.back(ctx),
            Action::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn back(&mut self, ctx: &egui::Context) {
        let _ = ctx;
        if self.pages.len() > 1 {
            // Leaving the wizard keeps nothing (its "Not now").
            if self.pages.pop().is_some_and(|p| p.kind == PageKind::Wizard) { self.wizard.clear(); }
        }
    }

    fn nav(&mut self, delta: i32) {
        if let Some(p) = self.pages.last_mut() {
            let n = p.items.len() as i32;
            if n > 0 {
                p.sel = ((p.sel as i32 + delta).rem_euclid(n)) as usize;
            }
        }
    }

    /// Controller and keyboard: up / down move, A / Enter / Space open, B / Esc / Backspace go back.
    /// Keys and pad as SkateLauncher.cs reads them (ProcessCmdKey / Poll / Act): arrows and W / S / A / D move, Enter
    /// selects, Esc / Backspace / Space go back; the pad (A, B, d-pad, left stick past 16000 of 32767) only while the
    /// window has the focus (every process sees the pad), up / down repeat when held; the same action from the other
    /// source within 150 ms is dropped (Steam can send a key for a press the pad also reports).
    fn input(&mut self, ctx: &egui::Context) -> (i32, bool, bool) {
        let mut acts: Vec<(u8, bool)> = Vec::new();
        ctx.input(|i| {
            let k = |keys: &[egui::Key]| keys.iter().any(|x| i.key_pressed(*x));
            if k(&[egui::Key::ArrowUp, egui::Key::W]) { acts.push((1, false)); }
            if k(&[egui::Key::ArrowDown, egui::Key::S]) { acts.push((2, false)); }
            if k(&[egui::Key::ArrowLeft, egui::Key::A]) { acts.push((3, false)); }
            if k(&[egui::Key::ArrowRight, egui::Key::D]) { acts.push((4, false)); }
            if k(&[egui::Key::Enter]) { acts.push((5, false)); }
            if k(&[egui::Key::Escape, egui::Key::Backspace, egui::Key::Space]) { acts.push((6, false)); }
        });
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let mut bits = 0u8;
        if let Some(g) = &mut self.gilrs {
            while g.next_event().is_some() {}
            if let Some((_, pad)) = g.gamepads().next() {
                const T: f32 = 16000.0 / 32767.0;
                let on = |b: gilrs::Button| pad.is_pressed(b);
                let (lx, ly) = (pad.value(gilrs::Axis::LeftStickX), pad.value(gilrs::Axis::LeftStickY));
                if on(gilrs::Button::DPadUp) || ly > T { bits |= 1; }
                if on(gilrs::Button::DPadDown) || ly < -T { bits |= 2; }
                if on(gilrs::Button::DPadLeft) || lx < -T { bits |= 4; }
                if on(gilrs::Button::DPadRight) || lx > T { bits |= 8; }
                if on(gilrs::Button::South) { bits |= 16; }
                if on(gilrs::Button::East) { bits |= 32; }
            }
        }
        if !focused { bits = 0; }
        let pressed = bits & !self.pad_prev;
        if pressed & 16 != 0 { acts.push((5, true)); }
        else if pressed & 32 != 0 { acts.push((6, true)); }
        else if pressed & 1 != 0 { acts.push((1, true)); }
        else if pressed & 2 != 0 { acts.push((2, true)); }
        else if pressed & 4 != 0 { acts.push((3, true)); }
        else if pressed & 8 != 0 { acts.push((4, true)); }
        else if bits & 3 != 0 && bits == self.pad_prev {
            // Held up / down: after about 0.22 s, a step every 64 ms (14 then every 4 frames of the C# 16 ms timer).
            let held = self.pad_since.elapsed();
            if held > Duration::from_millis(224) && self.last_nav.elapsed() >= Duration::from_millis(64) {
                self.last_nav = Instant::now();
                acts.push((if bits & 1 != 0 { 1 } else { 2 }, true));
            }
        }
        if bits != self.pad_prev { self.pad_since = Instant::now(); }
        self.pad_prev = bits;
        let (mut d, mut a, mut b) = (0, false, false);
        for (act, from_pad) in acts {
            if self.last_act.0 == act && self.last_act.1 != from_pad && self.last_act.2.elapsed() < Duration::from_millis(150) { continue; }
            self.last_act = (act, from_pad, Instant::now());
            match act {
                1 | 3 => d -= 1,
                2 | 4 => d += 1,
                5 => a = true,
                _ => b = true,
            }
        }
        (d, a, b)
    }

    fn job_frame(&mut self, ctx: &egui::Context, b: bool) {
        let Some(job) = &mut self.job else { return };
        let (done, waiting, can_cancel, cancel_text) = job.state.lock().map(|s| {
            let can = (s.waiting.is_some() && s.ended.is_none()) || s.fetch_waiting();
            let text = match (&s.prewait, s.build_start) {
                (Some((what, since)), _) => format!("Cancelled after {} while {what}. Nothing was built or changed.", clock(since.elapsed())),
                (None, Some(start)) => format!("Build cancelled after {} (it was waiting for another build).{}", clock(start.elapsed()),
                    s.log_path.as_ref().map(|p| format!("\nBuild log: {p}")).unwrap_or_default()),
                _ => "Cancelled.".into(),
            };
            (s.done, s.waiting.clone(), can, text)
        }).unwrap_or((None, None, false, String::new()));
        if b && can_cancel {
            job.cancel_presses += 1;
            if job.cancel_presses >= 2 {
                job.cancelled = Some(cancel_text);
                if let Ok(mut g) = job.child.lock() {
                    if let Some(c) = g.as_mut() {
                        kill_tree(c.id());
                        let _ = c.kill();
                    }
                }
            }
        }
        // The process scan is costly: four times a second is enough (C# polls at the same rate).
        if self.games_checked.elapsed() > Duration::from_millis(250) {
            self.games_checked = Instant::now();
            self.games_running = !crate::play::running_games().is_empty();
        }
        let running = self.games_running;
        let now = self.opened.elapsed().as_secs_f32();
        if running && !self.hidden && done.is_none() && waiting.is_none() {
            self.hidden = true;
            self.folding = true;
            self.anim.resume_at = None;
            self.anim.fold_at = Some(now);
            self.anim.roll();
        }
        if done.is_some() {
            let (close, cancelled, state) = (job.close_after, job.cancelled.clone(), job.state.clone());
            if self.hidden {
                self.folding = false;
                self.anim.fold_at = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.anim.resume_at = Some(now);
                self.anim.roll();
                self.hidden = false;
            }
            // An update or a switch of launchers during the job: start the new window (SkateLauncher.cs JobDone).
            if self.direct.is_empty() && self.restart_if_updated(true) {
                self.job = None;
                return;
            }
            let mut text = self.last_session();
            let mut mono = None;
            if let Some(c) = cancelled {
                text = c.lines().map(str::to_owned).collect();
            } else if let Ok(s) = state.lock() {
                if let (Some(path), Some(ok), Some(start), Some(end)) = (&s.log_path, s.ended, s.build_start, s.build_end) {
                    let took = clock(end - start);
                    text.push(String::new());
                    if ok {
                        let warn = if s.warnings > 0 { format!(", {} warnings", s.warnings) } else { String::new() };
                        text.push(format!("Built {} at {} in {took}{warn}.", s.building.clone().unwrap_or_default(), s.commit));
                        text.push(format!("Build log: {path}"));
                    } else {
                        text.push(format!("Build failed after {took}. Build log: {path}"));
                        text.push(String::new());
                        text.push("First error:".into());
                        mono = Some(s.first_error.join("\n"));
                    }
                }
            }
            let bad = mono.is_some() || text.iter().any(|l| ["MALFORMED", "No new", "Not started", "panic(s)", "failed", "Still missing"].iter().any(|w| l.contains(w)));
            self.pages.push(text_page(if bad { "Finished, check this" } else { "Finished" }, text, mono));
            self.job = None;
            self.relist();
            if let Some(after) = close {
                self.direct_close = Some(Instant::now() + after);
            }
        }
    }
}

fn text_page(title: &str, body: Vec<String>, mono: Option<String>) -> Page {
    Page { kind: PageKind::Text, title: title.into(), body, mono, ..Default::default() }
}

/// `FreeOn`: "Free on K:\: 1035 GB." for the drive holding `path` (empty when unknown).
fn free_on(path: &str) -> String {
    if path.is_empty() { return String::new(); }
    let p = std::path::Path::new(path);
    let root = p.ancestors().last().map(|r| r.display().to_string()).unwrap_or_default();
    match crate::window::free_bytes(p) {
        Some(b) => format!("Free on {}: {} GB.", if cfg!(windows) { p.components().next().map(|c| format!("{}\\", c.as_os_str().to_string_lossy())).unwrap_or(root) } else { root }, b >> 30),
        None => String::new(),
    }
}

/// The first file called `name` under `dir`, at most `depth` folders down.
fn find_file(dir: &std::path::Path, name: &str, depth: u32) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() { dirs.push(p); } else if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)) { return Some(p); }
    }
    if depth == 0 { return None; }
    dirs.into_iter().find_map(|d| find_file(&d, name, depth - 1))
}

/// `BuildView.Clock`: m:ss.
fn clock(d: Duration) -> String {
    format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60)
}

/// Escape sequences and carriage-return overdraws out of a build line (`BuildView.Clean`).
fn clean(line: &str) -> String {
    let line = line.rsplit('\r').next().unwrap_or(line);
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() { break; }
                }
            }
            continue;
        }
        if c == '\t' { out.push_str("    "); } else if c >= ' ' { out.push(c); }
    }
    out.trim_end().to_owned()
}

/// Ends a job's whole process tree (a build's cargo, a game).
fn kill_tree(pid: u32) {
    if cfg!(windows) {
        let _ = crate::proc::cmd("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).output();
    } else {
        let _ = crate::proc::cmd("pkill").args(["-TERM", "-P", &pid.to_string()]).output();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.bench_frame(ctx, frame);
        ctx.request_repaint_after(Duration::from_millis(100));
        if !self.started {
            self.started = true;
            self.begin_start();
        }
        if let Some(c) = self.restarted.as_mut() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            if c.try_wait().ok().flatten().is_some() {
                self.powered_off = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        if let Some(st) = self.start.as_mut() {
            let code = st.result.lock().ok().and_then(|g| *g);
            if let Some(code) = code { self.finish_start(code); }
        }
        let now = self.opened.elapsed().as_secs_f32();
        if self.anim.on_at.is_none() {
            self.anim.on_at = Some(now);
            if self.snapshot.is_none() { self.anim.roll(); }
        }
        // Closing (POWER, Exit, the window's own close) plays the power-off first.
        if self.snapshot.is_none() && !self.powered_off && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.anim.off_at.is_none() {
                self.anim.fold_at = None;
                self.anim.resume_at = None;
                self.folding = false;
                self.anim.off_at = Some(now);
                self.anim.roll();
            }
        }
        if self.anim.off_at.is_some_and(|o| now - o >= crate::camcorder::OFF_TIME) && !self.powered_off {
            self.powered_off = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.folding && self.anim.fold_at.is_some_and(|f| now - f >= self.anim.fold_time()) {
            self.folding = false;
            self.anim.fold_at = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if self.anim.resume_at.is_some_and(|r| now - r >= self.anim.resume_time()) {
            self.anim.resume_at = None;
        }
        if self.anim.animating(now) {
            ctx.request_repaint();
        }
        if let Some((entry, label)) = self.waiting_for.clone() {
            if self.pages.last().is_none_or(|p| p.title != "Waiting") {
                self.waiting_for = None;
            } else if self.games_checked.elapsed() > Duration::from_millis(250) {
                self.games_checked = Instant::now();
                if crate::play::running_games().is_empty() {
                    self.waiting_for = None;
                    self.pages.pop();
                    self.begin(&entry, &label);
                }
            }
        }
        if self.relist_pending && self.cache.lock().is_ok_and(|c| PREFETCHED.iter().all(|n| c.contains_key(*n))) {
            self.relist_pending = false;
            self.refresh_status();
            self.rebuild_main();
        }
        if self.fit_pending {
            if let (Some(m), Some(r)) = ctx.input(|i| (i.viewport().monitor_size, i.viewport().outer_rect)) {
                self.fit_pending = false;
                let (w, h) = crate::window::fit(m.x, m.y * 0.95);
                let c = r.center();
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(c.x - w / 2.0, c.y - h / 2.0)));
            }
        }
        if self.raise_checked.elapsed() > Duration::from_millis(400) {
            self.raise_checked = Instant::now();
            let raise = crate::window::raise_file(&self.state_dir);
            if raise.exists() {
                let _ = std::fs::remove_file(&raise);
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        if self.direct_close.is_some_and(|t| Instant::now() >= t) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let (d, a, b) = self.input(ctx);
        if b { if let Some(st) = self.start.as_mut() { st.skip_press(); } }
        if self.direct_close.is_some() && self.job.is_none() && (d != 0 || a || b) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.job.is_some() {
            self.job_frame(ctx, b);
        } else {
            if d != 0 {
                self.nav(d);
            }
            if a {
                self.activate(ctx);
            }
            if b {
                self.back(ctx);
            }
        }
        let t = self.opened.elapsed().as_secs_f32();
        if let Some(out) = self.snapshot.clone() {
            // After the wear texture is in (it builds off the window thread).
            if t > 2.0 && !self.snapshot_asked && self.wear.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.snapshot_asked = true;
            }
            let shot = ctx.input(|i| i.events.iter().find_map(|e| match e { egui::Event::Screenshot { image, .. } => Some(image.clone()), _ => None }));
            if let Some(img) = shot {
                let (screen, ppp, inner) = ctx.input(|i| (i.content_rect(), i.pixels_per_point(), i.viewport().inner_rect));
                println!("SNAPSHOT_SIZE image {}x{} screen {:?} ppp {ppp} inner {inner:?}", img.size[0], img.size[1], screen.size());
                // The window's surface can lag its layout for a few frames (seen at 150 % scale and once at 100 %): take
                // the shot again until it has the layout's size, then crop or pad to it as a last resort.
                let want = [(screen.width() * ppp).round() as usize, (screen.height() * ppp).round() as usize];
                if img.size != want && self.snapshot_tries < 20 {
                    self.snapshot_tries += 1;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                } else {
                    let mut rgba = vec![0u8; want[0] * want[1] * 4];
                    for y in 0..want[1].min(img.size[1]) {
                        for x in 0..want[0].min(img.size[0]) {
                            let c = img.pixels[y * img.size[0] + x].to_array();
                            rgba[(y * want[0] + x) * 4..][..4].copy_from_slice(&c);
                        }
                    }
                    if img.size != want { println!("SNAPSHOT_FIT {}x{} -> {}x{}", img.size[0], img.size[1], want[0], want[1]); }
                    let _ = image::save_buffer(&out, &rgba, want[0] as u32, want[1] as u32, image::ColorType::Rgba8);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            ctx.request_repaint();
        }
        if t < 1.5 {
            ctx.request_repaint();
        }
        if self.wear.is_none() && self.wear_rx.is_none() {
            // The sticker stays the same per install; the wear rolls a new seed every launch (user 2026-11-10). A
            // snapshot uses its seed for both. Built off the window's thread (about 0.4 s): the window shows at once and
            // the wear is in by the time the panel has swung open.
            let snap = self.snapshot.is_some().then_some(self.snapshot_seed);
            let state_dir = self.state_dir.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let spec = crate::wear::sticker_spec(&state_dir, snap);
                let marks_seed = snap.unwrap_or_else(crate::wear::launch_seed);
                let built = Instant::now();
                let px = crate::wear::render(&spec, marks_seed, Some(&state_dir.join("stickers")));
                let _ = tx.send((px, built.elapsed().as_secs_f32() * 1000.0));
            });
            self.wear_rx = Some(rx);
        }
        if let Some((px, ms)) = self.wear_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.wear_rx = None;
            self.wear_ms = ms;
            let img = egui::ColorImage::from_rgba_premultiplied([px.width() as usize, px.height() as usize], px.data());
            self.wear = Some(ctx.load_texture("wear", img, egui::TextureOptions::LINEAR));
        }
        if self.wear_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        let wear_id = self.wear.as_ref().map(|t| t.id());
        let mut hits = crate::camcorder::Hits::default();
        let splash = self.start.as_ref().map(StartCheck::stage_text);
        let status = |k: &str| self.status.iter().find(|(x, _)| x == k).map_or(String::new(), |(_, v)| v.clone());
        let (version, version_status, last, send) = (status("version"), status("version_status"), status("last"), status("send"));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            let job_state = self.job.as_ref().and_then(|j| j.state.lock().ok().map(|s| (j, s)));
            let job_view = job_state.as_ref().map(|(j, s)| crate::camcorder::JobView {
                title: &j.title, building: s.building.as_deref(), commit: &s.commit, progress: s.progress, compiled: s.compiled,
                warnings: s.warnings, now: &s.krate, waiting: s.waiting.as_deref(), stage: &s.stage, ended: s.ended,
                wait_who: s.wait_who.as_deref(), wait_secs: s.wait_since.map_or(0.0, |w| w.elapsed().as_secs_f32()),
                fetch_wait: s.fetch_waiting().then(|| s.prewait.as_ref().map(|(w, since)| (w.as_str(), since.elapsed().as_secs_f32()))).flatten(),
                built_secs: match (s.build_start, s.build_end) { (Some(a), Some(b)) => Some((b - a).as_secs_f32()), _ => None },
                error: &s.first_error, lines: s.lines.iter().map(String::as_str).collect(), elapsed: j.started.elapsed().as_secs_f32(),
            });
            let empty = Page::default();
            let page = self.pages.last().unwrap_or(&empty);
            let item = page.items.get(page.sel);
            let body = (!page.body.is_empty()).then(|| page.body.join("\n"));
            let view = crate::camcorder::View {
                main: page.kind == PageKind::Main, title: &page.title,
                items: page.items.iter().map(|i| (i.text.as_str(), i.on)).collect(), sel: page.sel,
                body, mono: page.mono.clone(),
                info: item.map_or("", |i| i.info.as_str()), hint: item.map_or("", |i| i.hint.as_str()),
                records: item.map_or(&[][..], |i| i.records.as_slice()),
                version: &version, version_status: &version_status, last: &last, send: &send,
                recording: job_view.is_some(), tape: job_state.as_ref().map_or(0.0, |(j, _)| j.started.elapsed().as_secs_f32()),
                job: job_view, splash: splash.as_deref(),
                splash_secs: self.start.as_ref().map_or(0.0, |s| s.since.elapsed().as_secs_f32()),
                splash_skip: self.start.as_ref().and_then(|s| s.can_skip().then_some(s.skip_armed)), hover: self.hover, cancel_armed: self.job.as_ref().is_some_and(|j| j.cancel_presses == 1),
                close_in: self.direct_close.map(|c| c.saturating_duration_since(Instant::now()).as_secs() as u32),
            };
            hits = crate::camcorder::paint(ui, &view, t, self.anim.panel_open(t), self.anim.screen_level(t), wear_id);
        });
        self.hover = hits.hover;
        if self.job.is_none() {
            let moved = ctx.input(|i| i.pointer.is_moving());
            if let (true, Some(i), Some(p)) = (moved, hits.hover_item, self.pages.last_mut()) {
                if p.sel != i && i < p.items.len() { p.sel = i; }
            }
            if hits.right_click { self.back(ctx); }
            if hits.empty_click && self.direct_close.is_some() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        }
        if hits.drag {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        use crate::camcorder::Key;
        match (hits.key, self.job.is_some()) {
            (Some(Key::Power), _) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            (Some(Key::Menu), false) => self.back(ctx),
            (Some(Key::Menu), true) => self.job_frame(ctx, true),
            (Some(Key::Up | Key::Left), false) => self.nav(-1),
            (Some(Key::Down | Key::Right), false) => self.nav(1),
            (Some(Key::A), false) => self.activate(ctx),
            (Some(Key::Play), false) if self.start.is_none() => {
                if let Some(p) = self.pages.last_mut().filter(|p| p.kind == PageKind::Main) { p.sel = 0; }
                self.activate(ctx);
            }
            _ => {}
        }
        if let (Some(i), None) = (hits.item, &self.job) {
            if let Some(p) = self.pages.last_mut() {
                p.sel = i;
            }
            self.activate(ctx);
        }
    }


    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

/// Opens the window. `direct`: launch options (`entry [label] [@version]`) that run at once and close afterwards.
#[allow(clippy::too_many_arguments)]
pub fn run(state_dir: PathBuf, direct: Vec<String>, snapshot: Option<PathBuf>, snapshot_page: Option<String>, snapshot_seed: Option<i32>, same_monitor: bool, after_update: bool, snapshot_size: Option<(f32, f32)>, bench: Option<PathBuf>) -> Result<(), String> {
    let snapshot = snapshot.or_else(|| bench.is_some().then(PathBuf::new)).filter(|_| true);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // Snapshots render at the panel's own 1240 x 760; a real start sizes from the monitor (window.rs).
    let area = if snapshot.is_some() { None } else { crate::window::start_area(same_monitor) };
    let (w, h) = match area { Some((_, _, aw, ah)) => crate::window::fit(aw, ah), None => snapshot_size.unwrap_or((1240.0, 760.0)) };
    let mut viewport = egui::ViewportBuilder::default().with_title("Skate launcher").with_inner_size([w, h])
        .with_resizable(false).with_maximize_button(false).with_transparent(true).with_decorations(false);
    if let Some((ax, ay, aw, ah)) = area {
        viewport = viewport.with_position([ax + (aw - w) / 2.0, ay + (ah - h) / 2.0]);
    }
    let fit_on_first_frame = snapshot.is_none() && area.is_none();
    let options = eframe::NativeOptions { viewport, centered: area.is_none(), ..Default::default() };
    eframe::run_native("Skate launcher", options, Box::new(move |cc| { crate::camcorder::install_fonts(&cc.egui_ctx); let mut app = App::new(exe, state_dir, direct); app.snapshot = snapshot; app.snapshot_seed = snapshot_seed.unwrap_or_else(crate::wear::launch_seed); app.snapshot_page(snapshot_page.as_deref()); if app.snapshot.is_some() { println!("SNAPSHOT_SEED {}", app.snapshot_seed); } app.fit_pending = fit_on_first_frame; app.same_monitor = same_monitor; app.after_update = after_update; if let Some(b) = bench { app.bench = Some((b, 0, Vec::new(), String::new())); app.snapshot = None; } Ok(Box::new(app)) })).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_log_is_written_from_the_stream_without_progress_redraws() {
        let dir = std::env::temp_dir().join(format!("skate-launcher-buildlog-{}", std::process::id()));
        let log = dir.join("logs").join("build.log");
        let mut s = JobState::default();
        feed(&mut s, &format!("LAUNCHER_BUILD\tbegin\thails-additions\tabc1234\t{}", log.display()));
        feed(&mut s, "   Compiling serde v1.0.0");
        feed(&mut s, "    Building [=====>    ] 10/20: serde");
        feed(&mut s, "\u{1b}[33mwarning\u{1b}[0m: unused variable");
        feed(&mut s, "LAUNCHER_BUILD\tend\t0");
        feed(&mut s, "after the build");
        let text = std::fs::read_to_string(&log).expect("log");
        assert_eq!(text.lines().collect::<Vec<_>>(), ["   Compiling serde v1.0.0", "warning: unused variable"]);
        assert_eq!(s.ended, Some(true));
        assert_eq!(s.progress, Some((10, 20)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_drops_escapes_and_overdraws() {
        assert_eq!(clean("old\rnew\u{1b}[1m bold\u{1b}[0m\tx"), "new bold    x");
    }
}
