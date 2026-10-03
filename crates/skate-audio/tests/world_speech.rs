//! The living world's speech rules on the dev install's export (`speech/livingworld.json` with the
//! `.evt` rules and clip ids, `audio_manifest.json` `world_tuning.speech_tuning`;
//! `.claude/skills/aems-port/tools/stage_world_audio.py`). Every test skips without that data.
//! Headless: no audio device, no game.
use std::collections::HashMap;
use std::path::PathBuf;

use skate_audio::world::speech::{Clip, SpeechIndex, Take, parse_name};
use skate_audio::world::speech_manager::{EventTuning, FAR, GateInputs, NEAR, Refusal, Speaker, SpeechManager, event_for_value, kind, speaker_bits};
use skate_audio::world::speech_rules::{ClipHeader, ClipRef, Event, EventTable, Library, Record};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A minimal JSON reader for our own export files (test support only).
#[derive(Clone, Debug)]
enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => &[],
        }
    }
    fn num(&self) -> f64 {
        match self {
            J::Num(n) => *n,
            J::Bool(b) => f64::from(u8::from(*b)),
            _ => 0.0,
        }
    }
    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => "",
        }
    }
    fn is_null(&self) -> bool {
        matches!(self, J::Null)
    }
}

fn parse_json(text: &str) -> J {
    fn ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
    }
    fn string(b: &[u8], i: &mut usize) -> String {
        *i += 1;
        let mut out = Vec::new();
        while b[*i] != b'"' {
            if b[*i] == b'\\' {
                *i += 1;
                match b[*i] {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        let code = u32::from_str_radix(std::str::from_utf8(&b[*i + 1..*i + 5]).unwrap(), 16).unwrap();
                        out.extend_from_slice(char::from_u32(code).unwrap_or('?').to_string().as_bytes());
                        *i += 4;
                    }
                    c => out.push(c),
                }
            } else {
                out.push(b[*i]);
            }
            *i += 1;
        }
        *i += 1;
        String::from_utf8_lossy(&out).into_owned()
    }
    fn value(b: &[u8], i: &mut usize) -> J {
        ws(b, i);
        match b[*i] {
            b'{' => {
                *i += 1;
                let mut v = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == b'}' {
                        *i += 1;
                        return J::Obj(v);
                    }
                    let k = string(b, i);
                    ws(b, i);
                    *i += 1; // ':'
                    v.push((k, value(b, i)));
                    ws(b, i);
                    if b[*i] == b',' {
                        *i += 1;
                    }
                }
            }
            b'[' => {
                *i += 1;
                let mut v = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == b']' {
                        *i += 1;
                        return J::Arr(v);
                    }
                    v.push(value(b, i));
                    ws(b, i);
                    if b[*i] == b',' {
                        *i += 1;
                    }
                }
            }
            b'"' => J::Str(string(b, i)),
            b't' => {
                *i += 4;
                J::Bool(true)
            }
            b'f' => {
                *i += 5;
                J::Bool(false)
            }
            b'n' => {
                *i += 4;
                J::Null
            }
            _ => {
                let start = *i;
                while *i < b.len() && matches!(b[*i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    *i += 1;
                }
                J::Num(std::str::from_utf8(&b[start..*i]).unwrap().parse().unwrap())
            }
        }
    }
    let mut i = 0;
    value(text.as_bytes(), &mut i)
}

struct Data {
    index: SpeechIndex,
    table: EventTable,
    headers: Vec<ClipHeader>,
    tuning: HashMap<u16, EventTuning>,
}

fn load() -> Option<Data> {
    let dir = root().join("assets/private/audio");
    let index = parse_json(&std::fs::read_to_string(dir.join("speech/livingworld.json")).ok()?);
    let rules = index.get("rules")?;
    let mut clips = Vec::new();
    let mut ids = Vec::new();
    let mut headers = Vec::new();
    for c in index.get("clips")?.arr() {
        let name = c.get("name")?.str().to_owned();
        let (event, voice, voice_name, line) = parse_name(&name)?;
        let takes: Vec<Take> = c.get("takes")?.arr().iter().map(|t| Take { offset: t.get("offset").map_or(0.0, J::num) as u32, size: t.get("size").map_or(0.0, J::num) as u32, rate: t.get("rate").map_or(0.0, J::num) as u32, samples: t.get("samples").map_or(0.0, J::num) as u32 }).collect();
        let id = c.get("id").filter(|j| !j.is_null()).map(|j| j.num() as u16);
        if let Some(id) = id {
            headers.push(ClipHeader { id, takes: takes.len() as u8, history: c.get("history").map_or(0.0, J::num) as u8, flags: 0 });
        }
        ids.push(id);
        clips.push(Clip { name, event, voice, voice_name, line, takes });
    }
    let mut index = SpeechIndex::new(clips);
    index.set_ids(&ids);
    let events = rules
        .get("events")?
        .arr()
        .iter()
        .map(|e| Event {
            id: e.get("id").unwrap().num() as u16,
            name: e.get("name").unwrap().str().to_owned(),
            queue_timeout: e.get("queue_timeout").unwrap().num() as u16,
            priority: e.get("priority").unwrap().num() as u16,
            conditions: e.get("conditions").unwrap().num() as u8,
            flags: e.get("flags").unwrap().num() as u8,
            probability: e.get("probability").unwrap().num() as u8,
            flags2: e.get("flags2").unwrap().num() as u8,
            fields: e.get("fields").unwrap().arr().iter().map(|f| f.num() as u8).collect(),
            records: e
                .get("records")
                .unwrap()
                .arr()
                .iter()
                .map(|r| Record {
                    weight_code: r.get("weight").unwrap().num() as u8,
                    probability: r.get("probability").unwrap().num() as u8,
                    mode: r.get("mode").unwrap().num() as u8,
                    locals: r.get("locals").unwrap().num() as u8,
                    values: r.get("values").unwrap().arr().iter().map(|v| v.num() as u32).collect(),
                    clips: r.get("clips").unwrap().arr().iter().map(|c| ClipRef { id: c.num() as u16, lookup: 0, params: 0 }).collect(),
                })
                .collect(),
        })
        .collect();
    let table = EventTable { bank: rules.get("bank")?.num() as u8, sub_bank: rules.get("sub_bank")?.num() as u8, events };
    let mut tuning = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(dir.join("audio_manifest.json"))
        && let Some(t) = parse_json(&text).get("world_tuning").and_then(|w| w.get("speech_tuning")).and_then(|s| s.get("1")).cloned()
        && let J::Obj(entries) = t
    {
        for (id, e) in entries {
            let f = |k: &str| e.get(k).map_or(0.0, J::num) as f32;
            tuning.insert(
                id.parse().unwrap(),
                EventTuning {
                    gap: f("gap"),
                    priority: f("priority") as i32,
                    probability: e.get("probability").map_or(100.0, J::num) as f32,
                    repeat: f("repeat"),
                    min_player_kmh: f("min_player_kmh"),
                    max_player_kmh: f("max_player_kmh"),
                    timer_40: f("timer_40"),
                    timer_44: f("timer_44"),
                    blocked_by: [false; 3],
                    zombie: e.get("zombie").is_some_and(|z| z.num() != 0.0),
                    not_follow: e.get("not_follow").map_or(&[][..], J::arr).iter().map(|p| (p.arr()[0].num() as u16, p.arr()[1].num() as f32)).collect(),
                    challenges: e.get("challenges").map_or(&[][..], J::arr).iter().map(|c| c.num() as i32).collect(),
                },
            );
        }
    }
    Some(Data { index, table, headers, tuning })
}

macro_rules! data {
    () => {
        match load() {
            Some(d) => d,
            None => panic!("missing private data: no speech rules export (stage_world_audio.py)"),
        }
    };
}

fn clip_name(d: &Data, id: u16) -> &str {
    d.index.clip_by_id(id).map_or("?", |i| d.index.clips[i].name.as_str())
}

#[test]
#[ignore = "needs the private install data"]
fn flag_one_names_the_far_lines_and_two_the_near_ones() {
    let d = data!();
    let (mut agree, mut against) = (0, 0);
    for ev in d.table.events.iter().filter(|e| matches!(e.id, 8202 | 8204 | 8207 | 8210 | 8220)) {
        assert_eq!(ev.fields, vec![1, 2, 3], "{}", ev.name);
        for r in &ev.records {
            for c in &r.clips {
                let line = clip_name(&d, c.id).trim_end_matches(".dat").to_ascii_lowercase();
                let far = line.ends_with("_f") || line.ends_with("_far");
                let near = line.ends_with("_n") || line.ends_with("_near");
                match (r.values[2], far, near) {
                    (1, true, false) | (2, false, true) => agree += 1,
                    (1, false, true) | (2, true, false) => against += 1,
                    _ => {}
                }
            }
        }
    }
    println!("near/far: {agree} records agree, {against} disagree");
    assert!(agree > 300);
    assert_eq!(against, 0);
}

#[test]
#[ignore = "needs the private install data"]
fn voices_map_to_their_type_and_variant_bits() {
    let d = data!();
    for (voice, bits) in [(59, (kind::BUSINESS_MAN, 1)), (41, (kind::ADULT_MALE, 1)), (49, (kind::ADULT_FEMALE, 8)), (53, (kind::TOURIST_FEMALE, 2)), (87, (kind::BUM, 1)), (90, (kind::SKATER_MALE, 0x10)), (76, (kind::SECURITY_GUARD, 2))] {
        assert_eq!(speaker_bits(&d.table, &d.index, voice), Some(bits), "voice {voice}");
    }
    // Every free-roam reaction event per speaker type: how many voices can say it.
    let mut rng = || 1u32;
    for value in [53, 25, 10, 23, 20, 2, 56, 6, 51] {
        let event = event_for_value(value, kind::JOCK, &mut rng).unwrap();
        let ev = d.table.event(event).unwrap();
        let voices: std::collections::BTreeSet<(u32, u32)> = ev.records.iter().filter(|r| r.values.len() >= 2).map(|r| (r.values[0], r.values[1])).collect();
        println!("value {value:2} → {} ({} records, {} speaker bit pairs)", ev.name, ev.records.len(), voices.len());
    }
}

#[test]
#[ignore = "needs the private install data"]
fn a_bumped_business_man_warns_far_or_near_and_cycles_his_takes() {
    let d = data!();
    let mut lib = Library::new(d.headers.clone());
    let mut manager = SpeechManager::new(d.tuning.clone());
    let warn = manager.tuning.get(&8210).cloned().unwrap_or_default();
    println!("501_warn tuning: repeat {} s, gap {} s, probability {} %, not-follow {:?}", warn.repeat, warn.gap, warn.probability, warn.not_follow);
    let busm1 = Speaker { index: 4, kind: kind::BUSINESS_MAN, variant: 1, ..Default::default() };
    let mut crt = skate_audio::world::Lcg(1);
    let mut now = 1000.0;
    let line = manager.request(&mut lib, &d.table, 53, FAR, &busm1, &GateInputs { now, ..Default::default() }, &mut crt).unwrap();
    assert_eq!(clip_name(&d, line.picks[0].clip), "501_59_busm1_Warn_f.dat");
    assert_eq!(manager.request(&mut lib, &d.table, 53, NEAR, &busm1, &GateInputs { now: now + 1.0, ..Default::default() }, &mut crt), Err(Refusal::Repeat));
    let clip = d.headers.iter().find(|h| clip_name(&d, h.id) == "501_59_busm1_Warn_n.dat").copied().unwrap();
    let mut takes = Vec::new();
    for _ in 0..usize::from(clip.takes) * 2 {
        now += f64::from(warn.repeat.max(warn.gap)) + 1.0;
        let line = manager.request(&mut lib, &d.table, 53, NEAR, &busm1, &GateInputs { now, ..Default::default() }, &mut crt);
        if let Ok(line) = line {
            assert_eq!(clip_name(&d, line.picks[0].clip), "501_59_busm1_Warn_n.dat");
            takes.push(line.picks[0].take);
        }
    }
    println!("busm1 Warn_n takes ({} takes, history {}): {takes:?}", clip.takes, clip.history);
    let n = usize::from(clip.takes);
    if takes.len() >= 2 * n && clip.history == clip.takes {
        let mut first = takes[..n].to_vec();
        first.sort_unstable();
        first.dedup();
        assert_eq!(first.len(), n, "every take once before a repeat");
        assert_eq!(takes[n..2 * n], takes[..n], "then the same order again");
    }
}

#[test]
#[ignore = "needs the private install data"]
fn every_free_roam_record_resolves_or_fails_closed() {
    let d = data!();
    let mut missing = 0;
    let mut total = 0;
    for ev in &d.table.events {
        assert_eq!(ev.conditions, 0, "{}", ev.name);
        for r in &ev.records {
            for c in &r.clips {
                total += 1;
                if d.index.clip_by_id(c.id).is_none() {
                    missing += 1;
                }
            }
        }
    }
    println!("{total} clip references, {missing} without a clip (their records never play)");
    assert!(missing * 10 < total);
    // The radio record: chirp, line, chirp, three different takes of the chirps' clip at most once
    // in a row.
    let mut lib = Library::new(d.headers.clone());
    let guard = Speaker { index: 1, kind: kind::SECURITY_GUARD, variant: 2, ..Default::default() };
    let mut manager = SpeechManager::new(d.tuning.clone());
    let mut crt = skate_audio::world::Lcg(7);
    let line = manager.request(&mut lib, &d.table, 1, NEAR, &guard, &GateInputs { now: 1000.0, ..Default::default() }, &mut crt).unwrap();
    let names: Vec<&str> = line.picks.iter().map(|p| clip_name(&d, p.clip)).collect();
    println!("guard radio: {names:?} takes {:?}", line.picks.iter().map(|p| p.take).collect::<Vec<_>>());
    assert_eq!(line.picks.len(), 3);
    assert_eq!(line.picks[0].clip, line.picks[2].clip);
    assert_ne!(line.picks[0].take, line.picks[2].take);
}
