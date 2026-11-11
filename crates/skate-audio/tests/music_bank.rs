//! A setup-written music bank against vgmstream's own decode of the same segment (data-gated: never committed data).
//! `SKATE3_MUSIC_BANK=<bank>` and `SKATE3_MUSIC_WAV=<the same segment decoded by vgmstream>` and
//! `SKATE3_MUSIC_SEGMENT=<1-based subsong>`; run with `cargo test -p skate-audio --test music_bank -- --ignored`.
use skate_audio::music::bank::{wav16, Bank};

#[test]
#[ignore = "requires a music bank and vgmstream's decode of one of its segments"]
fn a_bank_segment_matches_vgmstreams_decode_exactly() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
    let mut bank_file = std::fs::File::open(var("SKATE3_MUSIC_BANK")).unwrap();
    let bank = Bank::read_index(&mut bank_file).unwrap();
    let segment: usize = var("SKATE3_MUSIC_SEGMENT").parse().unwrap();
    let pcm = bank.decode(&mut bank_file, segment - 1).unwrap();
    let wav = wav16(&std::fs::read(var("SKATE3_MUSIC_WAV")).unwrap()).unwrap();
    assert_eq!((pcm.rate, pcm.channels.len() as u32), (wav.rate, wav.channels));
    let back: Vec<i16> = (0..pcm.channels[0].len())
        .flat_map(|f| pcm.channels.iter().map(move |c| (c[f] * 32768.0).round() as i16))
        .collect();
    assert_eq!(back.len(), wav.samples.len());
    assert!(back == wav.samples, "segment {segment} differs from vgmstream's decode");
    eprintln!("segment {segment}: {} frames match, bank of {} segments", back.len() / 2, bank.entries.len());
}
