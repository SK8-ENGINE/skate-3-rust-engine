//! Logging and Chrome-trace capture.
//!
//! `app.rs` disables Bevy's `LogPlugin`, so this module owns the global tracing
//! subscriber for the whole process. `init()` must therefore run before anything
//! logs, which is why `main` calls it first.
//!
//! Trace capture is opt-in via `--trace <path>`, with `--trace-wait` to arm it
//! for F9/F10 instead of recording from startup, and `--trace-seconds N` to stop
//! automatically. Bevy's `trace` feature supplies the spans we record.
use bevy::log::tracing::{Id, Subscriber, span::Attributes};
use bevy::log::tracing_subscriber::{
    EnvFilter, Layer, layer::Context, layer::SubscriberExt, registry::LookupSpan,
    util::SubscriberInitExt,
};
use bevy::prelude::*;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

/// Set once by `init()` so `install()` can reach the same capture state without
/// threading it through `main`'s return type.
static CAPTURE: OnceLock<&'static Capture> = OnceLock::new();

struct Options {
    output: PathBuf,
    wait: bool,
    gpu: bool,
    seconds: Option<f32>,
}

fn options() -> Result<Option<Options>, String> {
    let args: Vec<String> = std::env::args().collect();
    let Some(index) = args.iter().position(|a| a == "--trace") else {
        // The modifiers are meaningless alone; catching that here avoids a run
        // that silently produces no trace.
        for stray in ["--trace-wait", "--trace-gpu", "--trace-seconds"] {
            if args.iter().any(|a| a == stray) {
                return Err(format!("{stray} requires --trace <path>"));
            }
        }
        return Ok(None);
    };
    let output = args
        .get(index + 1)
        .filter(|a| !a.starts_with("--"))
        .ok_or_else(|| "--trace requires an output path".to_string())?;
    let seconds = match args.iter().position(|a| a == "--trace-seconds") {
        Some(at) => Some(
            args.get(at + 1)
                .ok_or_else(|| "--trace-seconds requires a value".to_string())?
                .parse::<f32>()
                .map_err(|e| format!("--trace-seconds is not a number: {e}"))?,
        ),
        None => None,
    };
    if std::env::var_os("SKATE_PERF_REPORT").is_some() {
        return Err("--trace and SKATE_PERF_REPORT are mutually exclusive: \
                    tracing changes the frame cost the perf report is measuring"
            .into());
    }
    Ok(Some(Options {
        output: PathBuf::from(output),
        wait: args.iter().any(|a| a == "--trace-wait"),
        gpu: args.iter().any(|a| a == "--trace-gpu"),
        seconds,
    }))
}

/// Chrome trace sink. Events are appended as they happen; the JSON array is
/// closed when the guard drops.
struct Capture {
    started: Instant,
    recording: AtomicBool,
    /// Distinct from `recording`: once finished we must not reopen the array.
    closed: AtomicBool,
    events: Mutex<Option<BufWriter<File>>>,
    written: AtomicU64,
}

impl Capture {
    fn write(&self, name:&str, thread:u64, begin:Instant, end:Instant) {
        if !self.recording.load(Ordering::Relaxed) {return;}
        let event=serde_json::json!({"ph":"X","name":name,"cat":"skate","pid":1,"tid":thread,
            "ts":begin.saturating_duration_since(self.started).as_micros() as u64,
            "dur":end.saturating_duration_since(begin).as_micros() as u64});
        let mut sink=self.events.lock().unwrap_or_else(|e|e.into_inner());
        let Some(file)=sink.as_mut() else{return;};
        if self.written.fetch_add(1,Ordering::Relaxed)!=0 {let _=file.write_all(b",\n");}
        let _=serde_json::to_writer(file,&event);
    }

    fn metadata(&self, name:&str, value:&str) {
        let mut sink=self.events.lock().unwrap_or_else(|e|e.into_inner());
        let Some(file)=sink.as_mut() else{return;};
        if self.written.fetch_add(1,Ordering::Relaxed)!=0 {let _=file.write_all(b",\n");}
        let _=serde_json::to_writer(file,&serde_json::json!({"ph":"M","name":name,"cat":"skate","pid":1,"tid":0,"ts":0,"args":{"value":value}}));
    }

    fn frame_interval(&self, milliseconds: f64) {
        let mut sink = self.events.lock().unwrap_or_else(|e| e.into_inner());
        let Some(file) = sink.as_mut() else { return; };
        if self.written.fetch_add(1, Ordering::Relaxed) != 0 { let _ = file.write_all(b",\n"); }
        let _ = serde_json::to_writer(file, &serde_json::json!({
            "ph":"C", "name":"frame_interval_ms", "cat":"skate", "pid":1,
            "tid":THREAD_ID.with(|id|*id), "ts":self.started.elapsed().as_micros() as u64,
            "args":{"value":milliseconds}
        }));
    }

    fn finish(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.recording.store(false, Ordering::SeqCst);
        let mut sink = self.events.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut file) = sink.take() {
            let _ = write!(file, "\n]\n");
            let _ = file.flush();
        }
    }
}

thread_local! {
    /// Chrome groups rows by `tid`. Real thread ids are not portably numeric, so
    /// hand out our own dense ids in first-touch order.
    static THREAD_ID: u64 = NEXT_THREAD.fetch_add(1, Ordering::Relaxed);
}
static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);

thread_local! { static ENTERED:std::cell::RefCell<Vec<(Id,Instant)>>=const {std::cell::RefCell::new(Vec::new())}; }
struct TraceLabel(String);
impl bevy::log::tracing::field::Visit for TraceLabel {
    fn record_debug(&mut self,field:&bevy::log::tracing::field::Field,value:&dyn std::fmt::Debug) {
        use std::fmt::Write;let _=write!(&mut self.0," {}={value:?}",field.name());
    }
}

struct ChromeLayer {
    capture: &'static Capture,
}

impl<S> Layer<S> for ChromeLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs:&Attributes<'_>, id:&Id, ctx:Context<'_,S>) {
        let mut label=TraceLabel(attrs.metadata().name().to_owned());attrs.record(&mut label);
        if let Some(span)=ctx.span(id) {span.extensions_mut().insert(label);}
    }
    fn on_enter(&self, id:&Id, _ctx:Context<'_,S>) {
        if self.capture.recording.load(Ordering::Relaxed) {
            ENTERED.with(|stack|stack.borrow_mut().push((id.clone(),Instant::now())));
        }
    }
    fn on_exit(&self, id:&Id, ctx:Context<'_,S>) {
        let begin=ENTERED.with(|stack| {let mut stack=stack.borrow_mut();let at=stack.iter().rposition(|(entered,_)|entered==id)?;Some(stack.remove(at).1)});
        if let (Some(begin),Some(span))=(begin,ctx.span(id)) {
            let end=Instant::now();let extensions=span.extensions();
            let label=extensions.get::<TraceLabel>().map_or(span.name(),|n|n.0.as_str());
            self.capture.write(label,THREAD_ID.with(|id|*id),begin,end);
        }
    }

}

/// Held by `main` for the process lifetime; closes the JSON array on drop so a
/// trace remains loadable even after a clean exit or an early return.
pub(crate) struct Guard {
    capture: Option<&'static Capture>,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(capture) = self.capture {
            capture.finish();
            info!("SKATE_TRACE finished");
        }
    }
}

pub(crate) fn init() -> Result<Guard, String> {
    // `log`-crate records (wgpu, naga, winit) are routed into tracing by
    // `SubscriberInitExt::init` below, which installs a `LogTracer` itself.
    // Installing one here as well makes that call fail with `SetLoggerError`,
    // and `init` unwraps, so doing this "defensively" panics before the window
    // ever opens.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu=warn,naga=warn,bevy_render=info"));
    let stderr = bevy::log::tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false);

    let Some(options) = options()? else {
        // `try_init` rather than `init`: losing the race for the global
        // subscriber costs log formatting, and killing the process over log
        // formatting is never the right trade.
        if let Err(error) = bevy::log::tracing_subscriber::registry()
            .with(filter)
            .with(stderr)
            .try_init()
        {
            eprintln!("SKATE_TRACE could not install the log subscriber: {error}");
        }
        return Ok(Guard { capture: None });
    };

    let mut file = BufWriter::new(
        File::create(&options.output)
            .map_err(|e| format!("Could not create trace {:?}: {e}", options.output))?,
    );
    write!(file, "[\n").map_err(|e| format!("Could not write trace header: {e}"))?;

    // Leaked deliberately: the layer and the Bevy systems both need a 'static
    // borrow, and it lives until process exit regardless.
    let capture: &'static Capture = Box::leak(Box::new(Capture {
        started: Instant::now(),
        recording: AtomicBool::new(!options.wait),
        closed: AtomicBool::new(false),
        events: Mutex::new(Some(file)),
        written: AtomicU64::new(0),
    }));
    let _ = CAPTURE.set(capture);

    // Here a failure does matter: without this subscriber no spans reach the
    // capture, so the trace would be an empty file rather than a bad one.
    bevy::log::tracing_subscriber::registry()
        .with(filter)
        .with(stderr)
        .with(ChromeLayer { capture })
        .try_init()
        .map_err(|e| format!("Could not install the trace subscriber: {e}"))?;

    if options.gpu {
        // RFC 3 lists GPU timestamps as an instrumentation gap. Refusing to
        // pretend is better than emitting a trace with no GPU rows in it.
        warn!("--trace-gpu is not wired yet; capturing CPU spans only");
    }
    if options.wait {
        info!("SKATE_TRACE armed: F9 starts capture, F10 stops it");
    } else {
        info!("SKATE_TRACE recording to {:?}", options.output);
    }
    if let Some(seconds) = options.seconds {
        info!("SKATE_TRACE will stop automatically after {seconds}s of capture");
    }
    STOP_AFTER
        .set(options.seconds)
        .map_err(|_| "trace options initialised twice".to_string())?;
    Ok(Guard { capture: Some(capture) })
}

static STOP_AFTER: OnceLock<Option<f32>> = OnceLock::new();

/// Records the map identity into the trace so a capture is self-describing.
pub(crate) fn map_metadata(config: &crate::config::Config) {
    let Some(capture) = CAPTURE.get() else { return };
    let name = config
        .map_path
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "test-world".into());
    capture.metadata("map", &name);
    capture.metadata("map_fingerprint", &format!("{:016x}", config.map_fingerprint));
    capture.metadata("difficulty", config.difficulty.key());
}

pub(crate) fn install(app: &mut App) {
    if CAPTURE.get().is_none() {
        return;
    }
    app.add_systems(Update, controls);
}

fn controls(keys: Res<ButtonInput<KeyCode>>, time: Res<Time<Real>>, mut started: Local<Option<f32>>) {
    let Some(capture) = CAPTURE.get() else { return };
    if keys.just_pressed(KeyCode::F9) && !capture.closed.load(Ordering::Relaxed) && !capture.recording.load(Ordering::Relaxed) {
        capture.recording.store(true, Ordering::SeqCst);
        *started = Some(time.elapsed_secs());
        info!("SKATE_TRACE capture started");
    }
    if keys.just_pressed(KeyCode::F10) && capture.recording.load(Ordering::Relaxed) {
        capture.finish();
        info!("SKATE_TRACE capture stopped");
    }
    if capture.recording.load(Ordering::Relaxed) {
        capture.frame_interval(time.delta_secs_f64() * 1000.0);
        let begin = started.get_or_insert(time.elapsed_secs());
        if let Some(limit) = STOP_AFTER.get().copied().flatten() {
            if time.elapsed_secs() - *begin >= limit {
                capture.finish();
                info!("SKATE_TRACE capture stopped after {limit}s");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trace_has_complete_spans_dynamic_labels_and_escaped_metadata() {
        let path = std::env::temp_dir().join(format!("skate-trace-{}-{}.json", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut file = BufWriter::new(File::create(&path).unwrap());
        file.write_all(b"[\n").unwrap();
        let capture: &'static Capture = Box::leak(Box::new(Capture {
            started: Instant::now(), recording: AtomicBool::new(false), closed: AtomicBool::new(false),
            events: Mutex::new(Some(file)), written: AtomicU64::new(0),
        }));
        let subscriber = bevy::log::tracing_subscriber::registry().with(ChromeLayer { capture });
        let metadata = "map \"quoted\" \\ assets\nnext";
        capture.metadata("map", metadata);
        bevy::log::tracing::subscriber::with_default(subscriber, || {
            { let _ignored = bevy::log::tracing::info_span!("before_capture").entered(); }
            capture.recording.store(true, Ordering::SeqCst);
            let outer = bevy::log::tracing::info_span!("mods.callback", mod_id = "broken-bones", callback = "on_fixed_update");
            let _outer = outer.enter();
            { let _inner = bevy::log::tracing::info_span!("nested").entered(); }
        });
        capture.finish();
        capture.finish();
        let events: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        let rows = events.as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["args"]["value"], metadata);
        assert_eq!(rows[1]["name"], "nested");
        assert!(rows[2]["name"].as_str().unwrap().contains("broken-bones"));
        assert!(rows[2]["name"].as_str().unwrap().contains("on_fixed_update"));
        assert_eq!(rows[2]["ph"], "X");
        assert!(rows[2]["ts"].as_u64().unwrap() <= rows[1]["ts"].as_u64().unwrap());
        assert!(rows[2]["dur"].as_u64().unwrap() >= rows[1]["dur"].as_u64().unwrap());
    }
}
