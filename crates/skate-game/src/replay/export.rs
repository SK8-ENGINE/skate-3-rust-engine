//! Replay export ("Save to PC", ours): Media Foundation .mp4 when present (b383), else PNG frames + WAV.
//! Details: docs/hails-additions/37-instant-replay.md#replay-export-encoder
use std::path::{Path, PathBuf};

/// What an export writes (defaults ours: the game's 30 fps console cadence for the frames, 48 kHz stereo sound).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ExportSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_bitrate: u32,
    pub audio_rate: u32,
    pub audio_bitrate: u32,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { width: 1280, height: 720, fps: 30, video_bitrate: 12_000_000, audio_rate: 48_000, audio_bitrate: 192_000 }
    }
}

/// Where frames and sound go. Frames are top-down BGRA (`width x height x 4` bytes); sound is interleaved stereo i16.
pub(crate) trait VideoSink {
    fn frame(&mut self, bgra: &[u8]) -> Result<(), String>;
    fn audio(&mut self, stereo: &[i16]) -> Result<(), String>;
    /// Close the file; returns what was written (the .mp4, or the fallback folder).
    fn finish(self: Box<Self>) -> Result<PathBuf, String>;
}

/// The sink for `path` (an .mp4): Media Foundation, else the PNG + WAV folder next to it (`<stem>_frames`). The reason
/// a fallback was taken is logged.
pub(crate) fn open_sink(path: &Path, settings: &ExportSettings) -> Result<Box<dyn VideoSink>, String> {
    #[cfg(windows)]
    match mf::MfSink::open(path, settings) {
        Ok(sink) => return Ok(Box::new(sink)),
        Err(e) => bevy::log::warn!("REPLAY_EXPORT Media Foundation unavailable ({e}): PNG frames + WAV instead"),
    }
    let dir = path.with_file_name(format!("{}_frames", path.file_stem().and_then(|s| s.to_str()).unwrap_or("replay")));
    Ok(Box::new(PngWavSink::open(&dir, settings)?))
}

/// The fallback: `frame_00001.png`.. and `audio.wav`.
pub(crate) struct PngWavSink {
    dir: PathBuf,
    settings: ExportSettings,
    frames: u32,
    audio: Vec<i16>,
}

impl PngWavSink {
    pub fn open(dir: &Path, settings: &ExportSettings) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self { dir: dir.to_owned(), settings: *settings, frames: 0, audio: Vec::new() })
    }
}

impl VideoSink for PngWavSink {
    fn frame(&mut self, bgra: &[u8]) -> Result<(), String> {
        let (w, h) = (self.settings.width, self.settings.height);
        let rgba: Vec<u8> = bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        let image = image::RgbaImage::from_raw(w, h, rgba).ok_or("frame size does not match the export size")?;
        self.frames += 1;
        image.save(self.dir.join(format!("frame_{:05}.png", self.frames))).map_err(|e| e.to_string())
    }

    fn audio(&mut self, stereo: &[i16]) -> Result<(), String> {
        self.audio.extend_from_slice(stereo);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<PathBuf, String> {
        write_wav(&self.dir.join("audio.wav"), &self.audio, self.settings.audio_rate)?;
        Ok(self.dir)
    }
}

/// A 16-bit stereo PCM WAV.
pub(crate) fn write_wav(path: &Path, stereo: &[i16], rate: u32) -> Result<(), String> {
    let data = (stereo.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in stereo {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(windows)]
mod mf {
    use super::{ExportSettings, VideoSink};
    use std::{ffi::c_void, path::{Path, PathBuf}};
    use windows::{
        core::{Interface, GUID, HRESULT, PCWSTR},
        Win32::{
            Foundation::HMODULE,
            Media::MediaFoundation::*,
            System::{
                Com::{CoInitializeEx, COINIT_MULTITHREADED},
                LibraryLoader::{GetProcAddress, LoadLibraryW},
            },
        },
    };

    type Startup = unsafe extern "system" fn(u32, u32) -> HRESULT;
    type Shutdown = unsafe extern "system" fn() -> HRESULT;
    type Create1 = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
    type CreateAttributes = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;
    type CreateBuffer = unsafe extern "system" fn(u32, *mut *mut c_void) -> HRESULT;
    type CreateWriter = unsafe extern "system" fn(PCWSTR, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;

    /// The Media Foundation entry points, from mfplat.dll / mfreadwrite.dll.
    struct Mf {
        shutdown: Shutdown,
        media_type: Create1,
        sample: Create1,
        attributes: CreateAttributes,
        buffer: CreateBuffer,
        writer: CreateWriter,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    impl Mf {
        fn load() -> Result<(Self, Startup), String> {
            unsafe {
                let module = |name: &str| -> Result<HMODULE, String> {
                    let w = wide(name);
                    LoadLibraryW(PCWSTR(w.as_ptr())).map_err(|e| format!("{name}: {e}"))
                };
                let (plat, rw) = (module("mfplat.dll")?, module("mfreadwrite.dll")?);
                let proc = |m: HMODULE, name: &str| -> Result<unsafe extern "system" fn() -> isize, String> {
                    let c: Vec<u8> = name.bytes().chain(std::iter::once(0)).collect();
                    GetProcAddress(m, windows::core::PCSTR(c.as_ptr())).ok_or(format!("{name} missing"))
                };
                Ok((
                    Self {
                        shutdown: std::mem::transmute(proc(plat, "MFShutdown")?),
                        media_type: std::mem::transmute(proc(plat, "MFCreateMediaType")?),
                        sample: std::mem::transmute(proc(plat, "MFCreateSample")?),
                        attributes: std::mem::transmute(proc(plat, "MFCreateAttributes")?),
                        buffer: std::mem::transmute(proc(plat, "MFCreateMemoryBuffer")?),
                        writer: std::mem::transmute(proc(rw, "MFCreateSinkWriterFromURL")?),
                    },
                    std::mem::transmute(proc(plat, "MFStartup")?),
                ))
            }
        }

        fn media_type(&self) -> windows::core::Result<IMFMediaType> {
            let mut raw = std::ptr::null_mut();
            unsafe {
                (self.media_type)(&mut raw).ok()?;
                Ok(IMFMediaType::from_raw(raw))
            }
        }
    }

    pub(super) struct MfSink {
        mf: Mf,
        writer: IMFSinkWriter,
        video: u32,
        audio: u32,
        settings: ExportSettings,
        frames: i64,
        samples: i64,
        path: PathBuf,
    }

    fn pair(hi: u32, lo: u32) -> u64 {
        (u64::from(hi) << 32) | u64::from(lo)
    }

    impl MfSink {
        pub fn open(path: &Path, s: &ExportSettings) -> Result<Self, String> {
            let (mf, startup) = Mf::load()?;
            let e = |e: windows::core::Error| e.to_string();
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                startup(MF_VERSION, MFSTARTUP_FULL).ok().map_err(e)?;
                let mut raw = std::ptr::null_mut();
                (mf.attributes)(&mut raw, 1).ok().map_err(e)?;
                let attributes = IMFAttributes::from_raw(raw);
                attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1).map_err(e)?;
                let url = super::wide_path(path);
                let mut raw = std::ptr::null_mut();
                (mf.writer)(PCWSTR(url.as_ptr()), std::ptr::null_mut(), attributes.as_raw(), &mut raw).ok().map_err(e)?;
                let writer = IMFSinkWriter::from_raw(raw);
                let video_type = |subtype: &GUID, out: bool| -> windows::core::Result<IMFMediaType> {
                    let t = mf.media_type()?;
                    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                    t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
                    t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                    t.SetUINT64(&MF_MT_FRAME_SIZE, pair(s.width, s.height))?;
                    t.SetUINT64(&MF_MT_FRAME_RATE, pair(s.fps, 1))?;
                    t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))?;
                    if out {
                        t.SetUINT32(&MF_MT_AVG_BITRATE, s.video_bitrate)?;
                    } else {
                        // Top-down rows (a positive stride).
                        t.SetUINT32(&MF_MT_DEFAULT_STRIDE, s.width * 4)?;
                    }
                    Ok(t)
                };
                let audio_type = |subtype: &GUID, out: bool| -> windows::core::Result<IMFMediaType> {
                    let t = mf.media_type()?;
                    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                    t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
                    t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, s.audio_rate)?;
                    t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                    t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                    if out {
                        t.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, s.audio_bitrate / 8)?;
                    } else {
                        t.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?;
                        t.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, s.audio_rate * 4)?;
                    }
                    Ok(t)
                };
                let video = writer.AddStream(&video_type(&MFVideoFormat_H264, true).map_err(e)?).map_err(e)?;
                writer.SetInputMediaType(video, &video_type(&MFVideoFormat_RGB32, false).map_err(e)?, None).map_err(e)?;
                let audio = writer.AddStream(&audio_type(&MFAudioFormat_AAC, true).map_err(e)?).map_err(e)?;
                writer.SetInputMediaType(audio, &audio_type(&MFAudioFormat_PCM, false).map_err(e)?, None).map_err(e)?;
                writer.BeginWriting().map_err(e)?;
                Ok(Self { mf, writer, video, audio, settings: *s, frames: 0, samples: 0, path: path.to_owned() })
            }
        }

        fn write(&self, stream: u32, bytes: &[u8], time: i64, duration: i64) -> windows::core::Result<()> {
            unsafe {
                let mut raw = std::ptr::null_mut();
                (self.mf.buffer)(bytes.len() as u32, &mut raw).ok()?;
                let buffer = IMFMediaBuffer::from_raw(raw);
                let mut data = std::ptr::null_mut();
                buffer.Lock(&mut data, None, None)?;
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
                buffer.Unlock()?;
                buffer.SetCurrentLength(bytes.len() as u32)?;
                let mut raw = std::ptr::null_mut();
                (self.mf.sample)(&mut raw).ok()?;
                let sample = IMFSample::from_raw(raw);
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(time)?;
                sample.SetSampleDuration(duration)?;
                self.writer.WriteSample(stream, &sample)
            }
        }
    }

    /// Test only: the first video frame of `path` decoded back to top-down BGRA (the source reader with its video
    /// processor; `IMF2DBuffer::Lock2D` gives the top row and a signed pitch, so the orientation is unambiguous).
    #[cfg(test)]
    pub(super) fn decode_first_frame(path: &Path, width: u32, height: u32) -> Result<Vec<u8>, String> {
        type CreateReader = unsafe extern "system" fn(PCWSTR, *mut c_void, *mut *mut c_void) -> HRESULT;
        let e = |e: windows::core::Error| e.to_string();
        let (mf, startup) = Mf::load()?;
        unsafe {
            let w = wide("mfreadwrite.dll");
            let rw = LoadLibraryW(PCWSTR(w.as_ptr())).map_err(e)?;
            let name = b"MFCreateSourceReaderFromURL\0";
            let create: CreateReader = std::mem::transmute(GetProcAddress(rw, windows::core::PCSTR(name.as_ptr())).ok_or("no source reader")?);
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            startup(MF_VERSION, MFSTARTUP_FULL).ok().map_err(e)?;
            let mut raw = std::ptr::null_mut();
            (mf.attributes)(&mut raw, 1).ok().map_err(e)?;
            let attributes = IMFAttributes::from_raw(raw);
            attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).map_err(e)?;
            let url = super::wide_path(path);
            let mut raw = std::ptr::null_mut();
            create(PCWSTR(url.as_ptr()), attributes.as_raw(), &mut raw).ok().map_err(e)?;
            let reader = IMFSourceReader::from_raw(raw);
            let t = mf.media_type().map_err(e)?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(e)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).map_err(e)?;
            let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            reader.SetCurrentMediaType(stream, None, &t).map_err(e)?;
            for _ in 0..30 {
                let (mut sample, mut actual, mut flags, mut time) = (None, 0u32, 0u32, 0i64);
                reader.ReadSample(stream, 0, Some(&mut actual), Some(&mut flags), Some(&mut time), Some(&mut sample)).map_err(e)?;
                let Some(sample) = sample else { continue };
                let mut out = Vec::with_capacity((width * height * 4) as usize);
                let mut copy = |scan0: *const u8, pitch: isize| {
                    for y in 0..height as isize {
                        out.extend_from_slice(std::slice::from_raw_parts(scan0.offset(y * pitch), width as usize * 4));
                    }
                };
                let buffer = sample.GetBufferByIndex(0).map_err(e)?;
                if let Ok(two) = buffer.cast::<IMF2DBuffer>() {
                    let (mut scan0, mut pitch) = (std::ptr::null_mut(), 0i32);
                    two.Lock2D(&mut scan0, &mut pitch).map_err(e)?;
                    copy(scan0, pitch as isize);
                    two.Unlock2D().map_err(e)?;
                } else {
                    // A plain buffer: rows by the output type's default stride (negative = bottom-up).
                    let current = reader.GetCurrentMediaType(stream).map_err(e)?;
                    let stride = current.GetUINT32(&MF_MT_DEFAULT_STRIDE).map_or(width as i32 * 4, |v| v as i32) as isize;
                    let mut data = std::ptr::null_mut();
                    buffer.Lock(&mut data, None, None).map_err(e)?;
                    let top = if stride < 0 { data.offset(-stride * (height as isize - 1)) } else { data };
                    copy(top, stride);
                    buffer.Unlock().map_err(e)?;
                }
                let _ = (mf.shutdown)();
                return Ok(out);
            }
            Err("no video frame decoded".into())
        }
    }

    /// Test only: the audio of `path` decoded to 16-bit PCM, its RMS level and how many samples were read.
    #[cfg(test)]
    pub(super) fn audio_level(path: &Path) -> Result<(f64, usize), String> {
        type CreateReader = unsafe extern "system" fn(PCWSTR, *mut c_void, *mut *mut c_void) -> HRESULT;
        let e = |e: windows::core::Error| e.to_string();
        let (mf, startup) = Mf::load()?;
        unsafe {
            let w = wide("mfreadwrite.dll");
            let rw = LoadLibraryW(PCWSTR(w.as_ptr())).map_err(e)?;
            let name = b"MFCreateSourceReaderFromURL\0";
            let create: CreateReader = std::mem::transmute(GetProcAddress(rw, windows::core::PCSTR(name.as_ptr())).ok_or("no source reader")?);
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            startup(MF_VERSION, MFSTARTUP_FULL).ok().map_err(e)?;
            let url = super::wide_path(path);
            let mut raw = std::ptr::null_mut();
            create(PCWSTR(url.as_ptr()), std::ptr::null_mut(), &mut raw).ok().map_err(e)?;
            let reader = IMFSourceReader::from_raw(raw);
            let t = mf.media_type().map_err(e)?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(e)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(e)?;
            let stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
            reader.SetCurrentMediaType(stream, None, &t).map_err(e)?;
            let (mut sum, mut count) = (0f64, 0usize);
            loop {
                let (mut sample, mut actual, mut flags, mut time) = (None, 0u32, 0u32, 0i64);
                reader.ReadSample(stream, 0, Some(&mut actual), Some(&mut flags), Some(&mut time), Some(&mut sample)).map_err(e)?;
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    break;
                }
                let Some(sample) = sample else { continue };
                let buffer = sample.ConvertToContiguousBuffer().map_err(e)?;
                let (mut data, mut len) = (std::ptr::null_mut(), 0u32);
                buffer.Lock(&mut data, None, Some(&mut len)).map_err(e)?;
                let pcm = std::slice::from_raw_parts(data as *const i16, len as usize / 2);
                sum += pcm.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>();
                count += pcm.len();
                buffer.Unlock().map_err(e)?;
            }
            let _ = (mf.shutdown)();
            Ok(((sum / count.max(1) as f64).sqrt(), count))
        }
    }

    impl VideoSink for MfSink {
        fn frame(&mut self, bgra: &[u8]) -> Result<(), String> {
            let (w, h) = (self.settings.width as usize, self.settings.height as usize);
            if bgra.len() != w * h * 4 {
                return Err("frame size does not match the export size".into());
            }
            // 100 ns units.
            let duration = 10_000_000 / i64::from(self.settings.fps);
            self.write(self.video, bgra, self.frames * duration, duration).map_err(|e| e.to_string())?;
            self.frames += 1;
            Ok(())
        }

        fn audio(&mut self, stereo: &[i16]) -> Result<(), String> {
            if stereo.is_empty() {
                return Ok(());
            }
            let bytes: Vec<u8> = stereo.iter().flat_map(|s| s.to_le_bytes()).collect();
            let rate = i64::from(self.settings.audio_rate);
            let frames = (stereo.len() / 2) as i64;
            self.write(self.audio, &bytes, self.samples * 10_000_000 / rate, frames * 10_000_000 / rate).map_err(|e| e.to_string())?;
            self.samples += frames;
            Ok(())
        }

        fn finish(self: Box<Self>) -> Result<PathBuf, String> {
            unsafe {
                self.writer.Finalize().map_err(|e| e.to_string())?;
                let path = self.path.clone();
                let shutdown = self.mf.shutdown;
                drop(self);
                let _ = shutdown();
                Ok(path)
            }
        }
    }
}

/// A path as a NUL-terminated wide string (Windows APIs).
#[cfg(windows)]
pub(crate) fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two seconds of moving colour and a tone through `open_sink`: an .mp4 with Windows' encoder, else the PNG + WAV
    /// folder; either way something real is written (no window, no game).
    #[test]
    fn exports_frames_and_sound_to_a_file() {
        let dir = std::env::temp_dir().join(format!("skate-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = ExportSettings { width: 320, height: 180, fps: 30, ..Default::default() };
        let path = dir.join("replay.mp4");
        let mut sink = open_sink(&path, &settings).unwrap();
        for f in 0..60u32 {
            let frame: Vec<u8> = (0..settings.width * settings.height).flat_map(|i| [(i + f * 4) as u8, (f * 4) as u8, 128, 255]).collect();
            sink.frame(&frame).unwrap();
            let tone: Vec<i16> = (0..1600).flat_map(|n| {
                let v = ((f64::from(f * 1600 + n) * 440.0 * std::f64::consts::TAU / 48_000.0).sin() * 8000.0) as i16;
                [v, v]
            }).collect();
            sink.audio(&tone).unwrap();
        }
        let written = sink.finish().unwrap();
        // Upright and the right way round: a frame with a white top half decodes with a bright top and a dark bottom.
        #[cfg(windows)]
        if written.extension().is_some_and(|e| e == "mp4") {
            let path = dir.join("upright.mp4");
            let mut sink = open_sink(&path, &settings).unwrap();
            let (w, h) = (settings.width as usize, settings.height as usize);
            let frame: Vec<u8> = (0..w * h).flat_map(|i| if i / w < h / 2 { [255u8, 255, 255, 255] } else { [0, 0, 0, 255] }).collect();
            for _ in 0..10 {
                sink.frame(&frame).unwrap();
            }
            sink.finish().unwrap();
            let decoded = mf::decode_first_frame(&path, settings.width, settings.height).unwrap();
            // B, G, R only (the decoder's fourth byte is unused, 0).
            let row_mean = |y: usize| decoded[y * w * 4..(y + 1) * w * 4].chunks_exact(4).map(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])).sum::<u32>() / (w as u32 * 3);
            let (top, bottom) = (row_mean(h / 8), row_mean(h * 7 / 8));
            eprintln!("decoded top row mean {top}, bottom row mean {bottom}");
            assert!(top > 200 && bottom < 60, "top {top} bottom {bottom}");
        }
        let size = if written.is_dir() {
            assert!(written.join("frame_00060.png").is_file() && written.join("audio.wav").is_file());
            std::fs::metadata(written.join("audio.wav")).unwrap().len()
        } else {
            std::fs::metadata(&written).unwrap().len()
        };
        eprintln!("export wrote {} ({size} bytes)", written.display());
        assert!(size > 10_000, "{size}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Manual check (`SKATE_EXPORT_CHECK=<file.mp4>`, ignored by default): the first frame of an export as a PNG next to it.
#[cfg(all(test, windows))]
#[test]
#[ignore]
fn decode_an_export_for_a_look() {
    let Ok(path) = std::env::var("SKATE_EXPORT_CHECK") else { return };
    let (w, h) = (1280, 800);
    let frame = mf::decode_first_frame(Path::new(&path), w, h).unwrap();
    let rgba: Vec<u8> = frame.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
    image::RgbaImage::from_raw(w, h, rgba).unwrap().save(format!("{path}.png")).unwrap();
    let (rms, samples) = mf::audio_level(Path::new(&path)).unwrap();
    eprintln!("audio: {samples} samples, rms {rms:.1}");
}
