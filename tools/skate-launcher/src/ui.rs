//! The launcher window (doc 44a section 2), on every OS: a page stack driven by a controller (gilrs), the keyboard or
//! the mouse. Every action runs this program's own text commands as a child process with `SKATE_LAUNCHER_UI=2`, the
//! way SkateLauncher.exe runs launcher.ps1, so the window and the command line share one backend.
use std::path::PathBuf;
use std::process::{Command, Stdio};
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
    mono: bool,
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
}

struct Job {
    title: String,
    state: Arc<Mutex<JobState>>,
    child: Arc<Mutex<Option<std::process::Child>>>,
    started: Instant,
    cancel_presses: u8,
    close_after: Option<Duration>,
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
    direct_close: Option<Instant>,
}

/// Runs a backend command and returns its lines (short query commands).
fn query(exe: &PathBuf, args: &[&str]) -> Vec<String> {
    Command::new(exe).args(args).env("SKATE_LAUNCHER_UI", "2").stdin(Stdio::null()).output().ok()
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
            Some("begin") => s.building = Some(f.get(2).copied().unwrap_or_default().to_owned()),
            Some("wait") => s.waiting = Some(f.get(3).copied().unwrap_or_default().to_owned()),
            Some("waitend") => s.waiting = None,
            Some("end") => s.building = None,
            _ => {}
        },
        Some("LAUNCHER_WAIT") => match f.get(1).copied() {
            Some("begin") => s.stage = format!("Fetching {}", f.get(3).copied().unwrap_or_default()),
            _ => s.stage.clear(),
        },
        Some("LAUNCHER_STAGE") => s.stage = f.get(1).copied().unwrap_or_default().to_owned(),
        Some("LAUNCHER_SELF") => {}
        _ => {
            let t = line.trim_start();
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
                s.first_error.push(line.to_owned());
            }
            if t.starts_with("Blocking waiting for file lock") {
                s.waiting = Some(t.to_owned());
            }
            s.lines.push(line.to_owned());
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
            last_nav: Instant::now(), hidden: false, wizard: Vec::new(), direct_close: None,
        };
        if !direct.is_empty() {
            app.start_direct(direct);
            return app;
        }
        let pending = Command::new(&app.exe).arg("pending").env("SKATE_LAUNCHER_UI", "2").status().ok().and_then(|s| s.code());
        app.refresh_status();
        app.pages.push(app.main_page());
        if app.status_of("setup_missing").is_some_and(|m| !m.is_empty()) {
            let p = app.setup_page();
            app.pages.push(p);
        }
        if app.status_of("first_setup").is_some_and(|f| f == "1") {
            let p = app.wizard_page();
            app.pages.push(p);
        }
        if pending == Some(10) {
            let text = app.last_session();
            app.pages.push(text_page("Previous session", text, false));
        }
        app
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
            self.pages.push(text_page(&first, lines, true));
            self.direct_close = Some(Instant::now() + Duration::from_secs(15));
            return;
        }
        self.pages.push(text_page("Starting", vec![first.clone()], false));
        self.run_job(&first, cmd);
        if let Some(j) = &mut self.job {
            j.close_after = Some(Duration::from_secs(10));
        }
    }

    fn refresh_status(&mut self) {
        self.status = query(&self.exe, &["status"]).iter().filter_map(|l| l.split_once('\t').map(|(k, v)| (k.to_owned(), v.to_owned()))).collect();
    }

    fn status_of(&self, k: &str) -> Option<&str> {
        self.status.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
    }

    fn last_session(&self) -> Vec<String> {
        std::fs::read_to_string(self.state_dir.join("last_session.txt")).map(|t| t.trim_start_matches('\u{feff}').lines().map(str::to_owned).collect())
            .unwrap_or_else(|_| vec!["No session yet.".into()])
    }

    fn entries(&self) -> Vec<Vec<String>> {
        query(&self.exe, &["entries"]).iter().map(|l| split(l)).filter(|f| f.len() >= 9).collect()
    }

    fn main_page(&self) -> Page {
        let entries = self.entries();
        let has = |g: &str| entries.iter().any(|e| e[4] == g);
        let item = |text: &str, hint: &str, on: bool, action: Action| Item { text: text.into(), hint: hint.into(), on, action, ..Default::default() };
        let version = self.status_of("version").unwrap_or_default().to_owned();
        let launcher = self.status_of("launcher").unwrap_or_default();
        let newer = self.status_of("launcher_newer").unwrap_or_default();
        let title = if newer.is_empty() { format!("Skate launcher  {launcher}") } else { format!("Skate launcher  {launcher} (newer: {newer})") };
        Page {
            kind: PageKind::Main, title,
            items: vec![
                item(&format!("Play: {version}"), self.status_of("version_status").unwrap_or_default(), true, Action::Open(PageKind::Play)),
                item("Versions", "Choose which version Play starts.", true, Action::Open(PageKind::Versions)),
                item("What to test", "The selected version's notes.", true, Action::Job(vec!["notes".into()])),
                item("Recomp", if has("recomp") { "Skate 3 Recompiled with trace options." } else { "Not set up: recomp_entries in config.json." }, has("recomp"), Action::Open(PageKind::Recomp)),
                item("RPCS3", if has("retail") { "Retail Skate 3 in RPCS3." } else { "Not set up: rpcs3_entries in config.json." }, has("retail"), Action::Open(PageKind::Retail)),
                item("Tools", "Refresh, build, DLC, settings, setup, clear builds.", true, Action::Open(PageKind::Tools)),
                item("Last session", self.status_of("last").unwrap_or_default(), true, Action::Open(PageKind::Text)),
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
        if kind == PageKind::Tools {
            let more = [
                ("Opt-ins and settings", "The game's opt-ins and settings.", Action::Open(PageKind::OptIns)),
                ("Share sessions via GitHub", "Turn sharing your session logs on or off.", Action::Open(PageKind::Share)),
                ("Set up the launcher", "Game folder, build space, your Skate 3 disc, the recomp and RPCS3.", Action::Open(PageKind::Wizard)),
                ("Clear builds and build caches", "Free disk space.", Action::Open(PageKind::Clear)),
            ];
            items.extend(more.into_iter().map(|(t, h, a)| Item { text: t.into(), hint: h.into(), on: true, action: a, ..Default::default() }));
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
        let items = query(&self.exe, &["versions"]).iter().map(|l| split(l)).filter(|f| f.len() >= 4).map(|f| Item {
            text: format!("{}{}", if f[3] == "*" { "> " } else { "  " }, f[1]), hint: f[2].clone(), on: true, action: Action::SetVersion(f[0].clone()), ..Default::default()
        }).collect();
        Page { kind: PageKind::Versions, title: "Choose version".into(), items, ..Default::default() }
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
        let item = |t: &str, h: &str, a: Action| Item { text: t.into(), hint: h.into(), on: true, action: a, ..Default::default() };
        Page { kind: PageKind::Share, title: format!("Share sessions ({})", self.status_of("share").unwrap_or("off")), items: vec![
            item("Turn it on", "Each session's logs are packed and a GitHub issue opens on the version's repository.", Action::Job(vec!["share-on".into()])),
            item("Not now", "Turns it off.", Action::Job(vec!["share-off".into()])),
            item("What is shared", "The game logs and traces of the session; no game files, no personal files.", Action::None),
        ], ..Default::default() }
    }

    fn setup_page(&self) -> Page {
        let rows: Vec<Vec<String>> = query(&self.exe, &["setup-check"]).iter().map(|l| split(l)).filter(|f| f.len() >= 5).collect();
        let body = rows.iter().map(|f| format!("{}  {}  {}", if f[2] == "1" { "ok     " } else { "MISSING" }, f[1], if f[2] == "1" { f[3].as_str() } else { "" })).collect();
        Page { kind: PageKind::SetupCheck, title: "First-time setup".into(), body, items: vec![
            Item { text: "Install what is missing".into(), hint: "Installs the missing tools (one permission prompt).".into(), on: true, action: Action::Begin { entry: "tool-setup".into(), ask: false }, ..Default::default() },
            Item { text: "Not now".into(), hint: String::new(), on: true, action: Action::Back, ..Default::default() },
        ], ..Default::default() }
    }

    fn wizard_page(&mut self) -> Page {
        if self.wizard.is_empty() {
            self.wizard = query(&self.exe, &["config-get"]).iter().filter_map(|l| l.split_once('\t').map(|(k, v)| (k.to_owned(), v.to_owned()))).collect();
        }
        let get = |k: &str| self.wizard.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap_or_default();
        let now = |v: String| if v.is_empty() { "Now: not set".to_owned() } else { format!("Now: {v}") };
        let cap = get("build_cap_gb");
        let item = |t: &str, h: String, a: Action| Item { text: t.into(), hint: h, on: true, action: a, ..Default::default() };
        Page { kind: PageKind::Wizard, title: "Set up the launcher".into(), items: vec![
            item("Game folder", now(get("storage_root")), Action::Wizard("storage_root".into())),
            item("Space for builds", format!("Now: {}", if cap.is_empty() || cap == "0" { "no limit".into() } else { format!("{cap} GB") }), Action::Wizard("build_cap_gb".into())),
            item("Skate 3 disc", now(get("disc")), Action::Wizard("disc".into())),
            item("Skate 3 Recompiled", now(get("recomp.working_dir")), Action::Wizard("recomp".into())),
            item("RPCS3", now(get("rpcs3.exe")), Action::Wizard("rpcs3.exe".into())),
            item("RPCS3 Skate 3 game", now(get("rpcs3.game")), Action::Wizard("rpcs3.game".into())),
            item("Done", "Saves what changed.".into(), Action::WizardDone),
            item("Not now", String::new(), Action::Back),
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
                let p = p.display().to_string();
                self.wizard_set("storage_root", p.clone());
                self.wizard_set("target_root", format!("{p}{}builds", std::path::MAIN_SEPARATOR));
                self.wizard_set("build_cap_dir", p);
            },
            "build_cap_gb" => {
                let steps = [0, 50, 100, 150, 200, 300, 500];
                let cur: i64 = self.wizard.iter().find(|(k, _)| k == "build_cap_gb").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
                let next = steps.iter().position(|s| *s == cur).map_or(steps[1], |i| steps[(i + 1) % steps.len()]);
                self.wizard_set("build_cap_gb", next.to_string());
            }
            "disc" => if let Some(p) = crate::pick::file("Your Skate 3 ISO or default.xex", "Skate 3 disc|*.iso;default.xex|All files|*.*") { self.wizard_set("disc", p.display().to_string()) },
            "recomp" => if let Some(p) = crate::pick::folder("The Skate 3 Recompiled folder") {
                let exe = ["skate3.exe", "recomp.exe"].iter().map(|n| p.join(n)).find(|x| x.exists());
                self.wizard_set("recomp.working_dir", p.display().to_string());
                if let Some(e) = exe { self.wizard_set("recomp.exe", e.display().to_string()) }
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

    fn wizard_done(&mut self) {
        let before: Vec<(String, String)> = query(&self.exe, &["config-get"]).iter().filter_map(|l| l.split_once('\t').map(|(k, v)| (k.to_owned(), v.to_owned()))).collect();
        let mut changed = serde_json::Map::new();
        for (k, v) in &self.wizard {
            if matches!(k.as_str(), "standalone" | "default_root") || before.iter().any(|(bk, bv)| bk == k && bv == v) {
                continue;
            }
            let value = if k == "build_cap_gb" { serde_json::json!(v.parse::<i64>().unwrap_or(0)) } else { serde_json::json!(v) };
            changed.insert(k.clone(), value);
        }
        changed.insert("first_setup_done".into(), serde_json::json!(true));
        let file = self.state_dir.join("config-set.json");
        let _ = std::fs::write(&file, serde_json::Value::Object(changed).to_string());
        let _ = query(&self.exe, &["config-set", &file.to_string_lossy()]);
        self.pages.pop();
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
            PageKind::Play => self.entry_page(kind, "rust", "Play"),
            PageKind::Recomp => self.entry_page(kind, "recomp", "Recomp"),
            PageKind::Retail => self.entry_page(kind, "retail", "RPCS3"),
            PageKind::Tools => self.entry_page(kind, "tools", "Tools"),
            PageKind::OptIns | PageKind::Settings => self.settings_page(kind),
            PageKind::Versions => self.versions_page(),
            PageKind::Clear => self.clear_page(),
            PageKind::Share => self.share_page(),
            PageKind::SetupCheck => self.setup_page(),
            PageKind::Wizard => self.wizard_page(),
            PageKind::Text => text_page("Last session", self.last_session(), false),
            PageKind::Main | PageKind::Label => return,
        };
        self.pages.push(page);
    }

    fn run_job(&mut self, title: &str, args: Vec<String>) {
        let state = Arc::new(Mutex::new(JobState::default()));
        let child = Command::new(&self.exe).args(&args).env("SKATE_LAUNCHER_UI", "2").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let Ok(mut child) = child else {
            self.pages.push(text_page("Could not start", vec![format!("Could not start {}", self.exe.display())], false));
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
        self.job = Some(Job { title: title.into(), state, child, started: Instant::now(), cancel_presses: 0, close_after: None });
    }

    fn begin(&mut self, entry: &str, label: &str) {
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
                let lines = query(&self.exe, &args.iter().map(String::as_str).collect::<Vec<_>>());
                if args[0].starts_with("share-") {
                    self.refresh_status();
                    self.pages.pop();
                }
                self.pages.push(text_page(&item.text, lines, args[0] == "notes"));
            }
            Action::SetVersion(id) => {
                let _ = query(&self.exe, &["version-set", &id]);
                self.refresh_status();
                self.pages.pop();
                self.rebuild_main();
            }
            Action::Setting(key) => {
                let out = query(&self.exe, &["game-setting", &key]);
                let (kind, sel) = self.pages.last().map(|p| (p.kind, p.sel)).unwrap_or_default();
                let mut page = self.settings_page(kind);
                page.sel = sel;
                *self.pages.last_mut().expect("page") = page;
                if !out.is_empty() {
                    self.pages.push(text_page("Not changed", out, false));
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
        if self.pages.len() > 1 {
            self.pages.pop();
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
    fn input(&mut self, ctx: &egui::Context) -> (i32, bool, bool) {
        let (mut d, mut a, mut b) = (0, false, false);
        ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowRight) { d += 1; }
            if i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowLeft) { d -= 1; }
            if i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space) { a = true; }
            if i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Backspace) { b = true; }
        });
        if let Some(g) = &mut self.gilrs {
            while let Some(ev) = g.next_event() {
                if let gilrs::EventType::ButtonPressed(button, _) = ev.event {
                    match button {
                        gilrs::Button::DPadDown | gilrs::Button::DPadRight => d += 1,
                        gilrs::Button::DPadUp | gilrs::Button::DPadLeft => d -= 1,
                        gilrs::Button::South | gilrs::Button::Start => a = true,
                        gilrs::Button::East | gilrs::Button::Select => b = true,
                        _ => {}
                    }
                }
                if let gilrs::EventType::AxisChanged(gilrs::Axis::LeftStickY, v, _) = ev.event {
                    if v.abs() > 0.6 && self.last_nav.elapsed() > Duration::from_millis(150) {
                        d += if v < 0.0 { 1 } else { -1 };
                        self.last_nav = Instant::now();
                    }
                }
            }
        }
        (d, a, b)
    }

    fn job_frame(&mut self, ctx: &egui::Context, b: bool) {
        let Some(job) = &mut self.job else { return };
        let (done, waiting) = job.state.lock().map(|s| (s.done, s.waiting.clone())).unwrap_or((None, None));
        if b {
            job.cancel_presses += 1;
            if job.cancel_presses >= 2 {
                if let Ok(mut g) = job.child.lock() {
                    if let Some(c) = g.as_mut() {
                        kill_tree(c.id());
                        let _ = c.kill();
                    }
                }
            }
        }
        let running = !crate::play::running_games().is_empty();
        if running && !self.hidden && done.is_none() && waiting.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            self.hidden = true;
        }
        if let Some(code) = done {
            if self.hidden {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.hidden = false;
            }
            let (first_error, title, close) = job.state.lock().map(|s| (s.first_error.clone(), job.title.clone(), job.close_after)).unwrap_or_default();
            let mut text = self.last_session();
            if code != 0 && !first_error.is_empty() {
                text.push(String::new());
                text.push("First error:".into());
                text.extend(first_error);
            }
            let bad = text.iter().any(|l| ["MALFORMED", "No new", "Not started", "panic(s)", "failed", "Still missing"].iter().any(|w| l.contains(w)));
            self.pages.push(text_page(if bad { "Finished, check this" } else { &title }, text, code != 0));
            self.job = None;
            self.refresh_status();
            self.rebuild_main();
            if let Some(after) = close {
                self.direct_close = Some(Instant::now() + after);
            }
        }
    }
}

fn text_page(title: &str, body: Vec<String>, mono: bool) -> Page {
    Page { kind: PageKind::Text, title: title.into(), body, mono, ..Default::default() }
}

/// Ends a job's whole process tree (a build's cargo, a game).
fn kill_tree(pid: u32) {
    if cfg!(windows) {
        let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).output();
    } else {
        let _ = Command::new("pkill").args(["-TERM", "-P", &pid.to_string()]).output();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(100));
        if self.direct_close.is_some_and(|t| Instant::now() >= t) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let (d, a, b) = self.input(ctx);
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
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(job) = &self.job {
                let s = job.state.lock();
                let Ok(s) = s else { return };
                ui.heading(format!("REC  {}", job.title));
                ui.label(format!("{:.0} s", job.started.elapsed().as_secs_f32()));
                if let Some(w) = &s.waiting {
                    ui.separator();
                    ui.label(format!("WAITING: {w}"));
                    ui.label("B twice to cancel");
                }
                if !s.stage.is_empty() {
                    ui.label(&s.stage);
                }
                if let Some(name) = &s.building {
                    ui.label(format!("Building {name}"));
                    if let Some((a, b)) = s.progress {
                        ui.add(egui::ProgressBar::new(a as f32 / b.max(1) as f32).text(format!("{a}/{b}")));
                    }
                    ui.label(format!("{}  ({} compiled, {} warnings)", s.krate, s.compiled, s.warnings));
                }
                ui.separator();
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    for l in s.lines.iter().rev().take(30).rev() {
                        ui.monospace(l);
                    }
                });
                return;
            }
            let Some(page) = self.pages.last() else { return };
            ui.heading(&page.title);
            ui.separator();
            for line in &page.body {
                if page.mono { ui.monospace(line); } else { ui.label(line); }
            }
            let mut clicked = None;
            ui.columns(2, |cols| {
                for (i, item) in page.items.iter().enumerate() {
                    let text = egui::RichText::new(&item.text).size(18.0);
                    let text = if item.on { text } else { text.weak() };
                    if cols[0].selectable_label(i == page.sel, text).clicked() {
                        clicked = Some(i);
                    }
                }
                if let Some(item) = page.items.get(page.sel) {
                    cols[1].label(egui::RichText::new(if item.on { "READY" } else { "NOT AVAILABLE" }).strong());
                    cols[1].label(&item.hint);
                    if !item.info.is_empty() {
                        cols[1].separator();
                        cols[1].label(&item.info);
                    }
                    for r in &item.records {
                        match r.strip_prefix('#') {
                            Some(head) => { cols[1].label(egui::RichText::new(head).strong()); }
                            None => { cols[1].label(r); }
                        }
                    }
                }
            });
            if let Some(i) = clicked {
                if let Some(p) = self.pages.last_mut() {
                    p.sel = i;
                }
                self.activate(ctx);
            }
        });
    }
}

/// Opens the window. `direct`: launch options (`entry [label] [@version]`) that run at once and close afterwards.
pub fn run(state_dir: PathBuf, direct: Vec<String>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Skate launcher").with_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native("Skate launcher", options, Box::new(move |_| Ok(Box::new(App::new(exe, state_dir, direct))))).map_err(|e| e.to_string())
}
