//! Which of the music object's MixMap outputs (Global slot object 1, SFXObj_Music) follow the Music option
//! (Master.in1) and the speech ducks (Speech.in0 / in1 / in4): the stream level retail's music takes (doc 11 "Music
//! level"). `cargo run -p skate-audio --example music_outputs -- <MixMapSK8.mxb>`.
use skate_audio::mixmap::{keys, MixMap};

fn levels(m: &MixMap) -> Vec<i32> {
    (0..28).map(|id| m.level(keys::MUSIC, id)).collect()
}

fn settle(m: &mut MixMap, ticks: usize) {
    for _ in 0..ticks {
        m.tick(1.0 / 60.0);
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: music_outputs <MixMapSK8.mxb>");
    let bytes = std::fs::read(&path).expect("read the MixMap");
    let mut m = MixMap::from_bytes(&bytes).expect("parse the MixMap");
    for id in 1..=4 {
        m.set_input(keys::MASTER, id, 32767);
    }
    settle(&mut m, 120);
    let base = levels(&m);
    println!("all volumes full:      {base:?}");
    m.set_input(keys::MASTER, 1, 0);
    settle(&mut m, 120);
    println!("Music option 0:        {:?}", levels(&m));
    m.set_input(keys::MASTER, 1, 16384);
    settle(&mut m, 120);
    println!("Music option half:     {:?}", levels(&m));
    m.set_input(keys::MASTER, 1, 32767);
    for id in [0, 1, 4] {
        m.set_input(keys::SPEECH, id, 32767);
    }
    settle(&mut m, 240);
    println!("speech on (2 s):       {:?}", levels(&m));
    for id in [0, 1, 4] {
        m.set_input(keys::SPEECH, id, 0);
    }
    settle(&mut m, 600);
    println!("speech off (10 s):     {:?}", levels(&m));
}
