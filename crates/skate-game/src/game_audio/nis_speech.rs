//! Presentation speech playback: the lines the Speech / SpeechNoWait events picked (`presentation_run::SpeechPlay`)
//! play 2D on their own bank, one at a time, and raise the scripted-dialogue input of Obj:Speech while they play.
//! Research b1014: retail streams the line on the main cast's channel 0 with no actor or position, and MixMap
//! Obj:Speech input 2 follows the speech system's scripted state (which ducks the music).
//! Details: docs/hails-additions/42-challenges.md#presentation-speech
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use skate_audio::formats::SampleHeader;
use skate_audio::world::speech::NIS_BANK;

use super::native::Native;

type Loaded = Arc<Mutex<Option<Option<skate_audio::mixer::Pcm>>>>;

#[derive(Default)]
pub(super) struct NisVoice {
    /// The line being read on its thread (the result once read; None inside = unreadable).
    loading: Option<(PathBuf, Loaded)>,
    voice: Option<u32>,
}

pub(super) fn frame(presenter: Option<Res<crate::presentation_run::Presenter>>, native: Option<ResMut<Native>>,
                    speech: Option<ResMut<super::world_speech::WorldSpeech>>, mut state: Local<NisVoice>) {
    let Some(presenter) = presenter else { return };
    let plays = presenter.speech_out.lock().map(|mut o| std::mem::take(&mut *o)).unwrap_or_default();
    // One stream: the handler already refuses a request while a line plays; the newest wins otherwise.
    if let Some(play) = plays.into_iter().last() {
        let cell: Loaded = Arc::new(Mutex::new(None));
        let (path, result) = (play.file.clone(), cell.clone());
        let spawned = std::thread::Builder::new().name("nis-speech".into()).spawn(move || {
            let pcm = std::fs::read(&path).ok().and_then(|b| super::library::wav_pcm(&b));
            if let Ok(mut r) = result.lock() {
                *r = Some(pcm);
            }
        });
        if spawned.is_ok() {
            state.loading = Some((play.file, cell));
        }
    }
    let Some(native) = native else { return };
    let ready = state.loading.as_ref().and_then(|(_, cell)| cell.lock().ok().and_then(|mut c| c.take()));
    let mut playing = state.loading.is_some();
    if let Ok(mut rt) = super::timing::lock(&native.shared, &super::timing::GAME_LOCK) {
        if let Some(pcm) = ready {
            let (file, _) = state.loading.take().expect("ready implies loading");
            match pcm {
                Some(pcm) => {
                    let frames = pcm.channels.first().map_or(0, Vec::len) as u32;
                    let header = SampleHeader { codec: 0, channels: pcm.channels.len().clamp(1, 8) as u8, rate: pcm.rate, frames, loop_start: None };
                    rt.mixer.set_bank_sample(NIS_BANK, 0, header, Arc::new(pcm));
                    state.voice = rt.mixer.open_direct(NIS_BANK, 0, 0.0, 1.0, 1.0, None);
                    info!("AUDIO_NIS_SPEECH {} voice {:?}", file.display(), state.voice);
                }
                None => warn!("AUDIO_NIS_SPEECH {} not readable (run setup to export the presentation speech)", file.display()),
            }
        }
        playing = state.loading.is_some() || state.voice.is_some_and(|v| rt.mixer.direct_alive(v));
    }
    if !playing {
        state.voice = None;
    }
    if let Some(mut s) = speech {
        s.scripted = playing;
    }
}
