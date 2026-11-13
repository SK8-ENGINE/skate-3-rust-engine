//! Driving a replay export (M4, b383): Save to PC, lock-step video and audio frames to the encoder thread.
//! Details: docs/hails-additions/37-instant-replay.md#replay-export-run
use super::{export, Replay};
use bevy::prelude::*;
use std::{
    path::PathBuf,
    sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex},
    thread::JoinHandle,
};

/// While set, the audio device plays silence and leaves the runtime to the exporter (`game_audio::native`).
pub(crate) static DEVICE_MUTED: AtomicBool = AtomicBool::new(false);

/// The Finished menu's rows (ours: retail's Upload / Save Replay need sign-in and storage). Data.
pub(crate) const FINISHED_POPUP: &str = "engine:replay.finished";
/// Our own string id (not on the disc): its English text is a default in `apt_text::ENGINE_STRINGS`, so a language
/// table or mod text that carries the id replaces it.
pub(crate) const SAVE_TO_PC: &str = "ID_HAILS_REPLAY_SAVE_TO_PC";
pub(crate) const SAVED_POPUP: &str = "engine:replay.saved";
/// Retail asks before saving (b387 row 2: title = the row, ID_REPLAY_EDITOR_POPUP_REPLAYSAVE, No / Yes; callback
/// `82616000`: No closes back to the editor).
pub(crate) const SAVE_CONFIRM_POPUP: &str = "engine:replay.save_confirm";

pub(crate) fn save_confirm_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: SAVE_TO_PC.into(),
        description: "ID_REPLAY_EDITOR_POPUP_REPLAYSAVE".into(),
        options: vec!["ID_COMMON_NO".into(), "ID_COMMON_YES".into()],
        callback: SAVE_CONFIRM_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

pub(crate) fn finished_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_REPLAYEDITOR_TITLE".into(),
        description: "#".into(),
        options: vec![SAVE_TO_PC.into(), "ID_REPLAYEDITOR_EXIT".into()],
        callback: FINISHED_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

fn saved_popup(text: String) -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_REPLAYEDITOR_TITLE".into(),
        description: text,
        options: vec!["ID_COMMON_OK".into()],
        callback: SAVED_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

enum Msg {
    Frame(Vec<u8>),
    Audio(Vec<i16>),
}

struct Job {
    tx: Option<mpsc::SyncSender<Msg>>,
    handle: Option<JoinHandle<Result<PathBuf, String>>>,
    settings: export::ExportSettings,
    start: f64,
    frame: u32,
    total: u32,
    /// The readback entity of the current frame and where its data lands.
    readback: Option<(Entity, Arc<Mutex<Option<Vec<u8>>>>)>,
    /// Sound samples owed (fractional), stereo floats per pull.
    owed: f64,
}

/// The export in progress (none = idle).
#[derive(Resource, Default)]
pub(crate) struct ReplayExport {
    /// The save dialog's answer (a `Mutex`: a resource must be `Sync`).
    choosing: Option<Mutex<mpsc::Receiver<Option<PathBuf>>>>,
    job: Option<Job>,
}

impl ReplayExport {
    pub(crate) fn busy(&self) -> bool {
        self.choosing.is_some() || self.job.is_some()
    }

    /// Export to `path` without the dialog (verify runs, mods later).
    pub(crate) fn export_to(&mut self, path: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(Some(path));
        self.choosing = Some(Mutex::new(rx));
    }

    /// "Save to PC": ask for a file (the dialog runs on its own thread).
    pub(crate) fn choose(&mut self) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(save_dialog());
        });
        self.choosing = Some(Mutex::new(rx));
    }
}

#[cfg(windows)]
fn save_dialog() -> Option<PathBuf> {
    use windows::{
        core::{w, HSTRING},
        Win32::{
            System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
            UI::Shell::{Common::COMDLG_FILTERSPEC, FileSaveDialog, IFileSaveDialog, SIGDN_FILESYSPATH},
        },
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filters = [COMDLG_FILTERSPEC { pszName: w!("MP4 video"), pszSpec: w!("*.mp4") }];
        dialog.SetFileTypes(&filters).ok()?;
        dialog.SetDefaultExtension(w!("mp4")).ok()?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        dialog.SetFileName(&HSTRING::from(format!("skate-replay-{stamp}.mp4"))).ok()?;
        dialog.Show(None).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(name.0 as *const _));
        path
    }
}

#[cfg(not(windows))]
fn save_dialog() -> Option<PathBuf> {
    None
}

/// Rows of `width x 4` bytes from a readback padded to 256-byte rows, RGBA -> BGRA (the encoder's RGB32).
pub(crate) fn unpad_bgra(data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let row = width as usize * 4;
    let padded = row.div_ceil(256) * 256;
    if data.len() < padded * (height as usize - 1) + row {
        return None;
    }
    let mut out = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        out.extend(data[y * padded..y * padded + row].chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]));
    }
    Some(out)
}

/// The export state machine, every frame (after the replay's own systems).
pub(crate) fn drive(
    mut commands: Commands,
    mut export: ResMut<ReplayExport>,
    mut replay: ResMut<Replay>,
    target: Option<Res<crate::graphics_menu::SceneTarget>>,
    images: Res<Assets<Image>>,
    native: Option<Res<crate::game_audio::native::Native>>,
    mut popups: ResMut<crate::popup_host::PopupMods>,
) {
    // The file was chosen (or the dialog cancelled): start rendering the clip.
    let answer = export.choosing.as_ref().map(|rx| rx.lock().map_or(Err(mpsc::TryRecvError::Disconnected), |rx| rx.try_recv()));
    if let Some(answer) = answer {
        match answer {
            Ok(Some(path)) => {
                export.choosing = None;
                let Some(target) = target.as_ref() else { return };
                let size = images.get(&target.0).map(|i| i.size()).unwrap_or(UVec2::new(1280, 720));
                // H.264 wants even sizes; the readback takes the full image, the encoder its even part.
                let settings = export::ExportSettings { width: size.x & !1, height: size.y & !1, ..Default::default() };
                let (start, end) = replay.editor.as_ref().map_or(replay.bounds(), |e| e.heads());
                let total = ((end - start) * f64::from(settings.fps)).floor().max(1.0) as u32;
                let (tx, rx) = mpsc::sync_channel::<Msg>(8);
                let job_path = path.clone();
                let handle = std::thread::spawn(move || {
                    let mut sink = export::open_sink(&job_path, &settings)?;
                    while let Ok(msg) = rx.recv() {
                        match msg {
                            Msg::Frame(f) => sink.frame(&f)?,
                            Msg::Audio(a) => sink.audio(&a)?,
                        }
                    }
                    sink.finish()
                });
                info!("REPLAY_EXPORT start {} {}x{} {} frames ({:.1}s)", path.display(), settings.width, settings.height, total, end - start);
                DEVICE_MUTED.store(true, Ordering::Relaxed);
                replay.playing = false;
                replay.exporting = true;
                export.job = Some(Job { tx: Some(tx), handle: Some(handle), settings, start, frame: 0, total, readback: None, owed: 0.0 });
            }
            Ok(None) | Err(mpsc::TryRecvError::Disconnected) => export.choosing = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    let Some(job) = export.job.as_mut() else { return };
    // Every frame sent: wait for the encoder, then report.
    if job.frame >= job.total {
        job.tx = None;
        if job.handle.as_ref().is_some_and(|h| h.is_finished()) {
            let result = job.handle.take().map(|h| h.join().unwrap_or_else(|_| Err("encoder thread panicked".into())));
            let text = match result {
                Some(Ok(path)) => {
                    info!("REPLAY_EXPORT done {}", path.display());
                    format!("Saved to {}", path.display())
                }
                Some(Err(e)) => {
                    warn!("REPLAY_EXPORT failed: {e}");
                    format!("The video could not be saved: {e}")
                }
                None => String::new(),
            };
            popups.engine_requests.push(saved_popup(text));
            DEVICE_MUTED.store(false, Ordering::Relaxed);
            replay.exporting = false;
            export.job = None;
        }
        return;
    }
    let Some(target) = target else { return };
    match job.readback.take() {
        None => {
            // The next frame: the cursor, then a readback of what this frame renders.
            let time = job.start + f64::from(job.frame) / f64::from(job.settings.fps);
            replay.seek(time);
            let slot = Arc::new(Mutex::new(None));
            let landed = slot.clone();
            let entity = commands
                .spawn(bevy::render::gpu_readback::Readback::texture(target.0.clone()))
                .observe(move |event: On<bevy::render::gpu_readback::ReadbackComplete>| {
                    if let Ok(mut s) = landed.lock() {
                        s.get_or_insert_with(|| event.data.clone());
                    }
                })
                .id();
            job.readback = Some((entity, slot));
        }
        Some((entity, slot)) => {
            let data = slot.lock().ok().and_then(|mut s| s.take());
            let Some(data) = data else {
                job.readback = Some((entity, slot));
                return;
            };
            commands.entity(entity).despawn();
            let size = images.get(&target.0).map(|i| i.size()).unwrap_or_default();
            let (w, h) = (job.settings.width, job.settings.height);
            let frame = unpad_bgra(&data, size.x, size.y).map(|full| {
                // The even part (the encoder's size) of the full image.
                (0..h as usize).flat_map(|y| full[y * size.x as usize * 4..][..w as usize * 4].to_vec()).collect::<Vec<u8>>()
            });
            // One frame of sound from the runtime the replay feeds (256-sample blocks; the fraction is carried).
            job.owed += f64::from(job.settings.audio_rate) / f64::from(job.settings.fps);
            let mut audio = Vec::new();
            if let Some(native) = native.as_ref() {
                let mut block = vec![0.0f32; 512];
                while job.owed >= 256.0 {
                    job.owed -= 256.0;
                    if let Ok(mut runtime) = native.shared.lock() {
                        runtime.fill_stereo(&mut block);
                    }
                    audio.extend(block.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16));
                }
            }
            let tx = job.tx.as_ref();
            let sent = frame.is_some_and(|f| tx.is_some_and(|tx| tx.send(Msg::Frame(f)).is_ok()))
                && tx.is_some_and(|tx| tx.send(Msg::Audio(audio)).is_ok());
            if !sent {
                warn!("REPLAY_EXPORT frame {} not sent (size {size:?}, data {} bytes)", job.frame, data.len());
                job.frame = job.total;
                return;
            }
            job.frame += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readback_rows_are_unpadded_and_swapped_to_bgra() {
        // 3 x 2 RGBA: 12-byte rows padded to 256.
        let mut data = vec![0u8; 256 + 12];
        data[0..4].copy_from_slice(&[1, 2, 3, 4]);
        data[256..260].copy_from_slice(&[5, 6, 7, 8]);
        let out = unpad_bgra(&data, 3, 2).unwrap();
        assert_eq!(out.len(), 24);
        assert_eq!(&out[0..4], &[3, 2, 1, 255]);
        assert_eq!(&out[12..16], &[7, 6, 5, 255]);
        assert!(unpad_bgra(&data[..100], 3, 2).is_none());
    }
}
