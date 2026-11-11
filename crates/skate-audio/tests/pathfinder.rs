//! The disc's PathFinder graphs (data-gated, never committed): `SKATE3_MPF_DIR=<disc>/data/audio/music`;
//! run with `cargo test -p skate-audio --test pathfinder -- --ignored`. Expected values: research b1016.
use skate_audio::music::pathfinder::format::{Graph, Kind};
use skate_audio::music::pathfinder::stream::Stream;

fn graph(name: &str) -> Graph {
    let dir = std::env::var_os("SKATE3_MPF_DIR").expect("set SKATE3_MPF_DIR");
    Graph::parse(&std::fs::read(std::path::Path::new(&dir).join(name)).unwrap()).unwrap()
}

#[test]
#[ignore = "requires the disc's music graphs"]
fn the_three_graphs_parse_with_their_counts() {
    for (name, nodes, events, vars, samples, slot) in
        [("game.mpf", 1210, 15, 8, 1074, 1), ("ipod.mpf", 2116, 3, 3, 1380, 2), ("world.mpf", 5873, 5, 2, 5725, 0)] {
        let g = graph(name);
        assert_eq!((g.nodes.len(), g.events.len(), g.vars.len(), g.durations.len(), g.slot), (nodes, events, vars, samples, slot), "{name}");
    }
    let ipod = graph("ipod.mpf");
    assert_eq!((ipod.entries(1).len(), ipod.entries(2).len()), (46, 46));
    assert!(ipod.events.iter().any(|e| e.id == 0x0416_D750 & 0xFF_FFFF));
}

#[test]
#[ignore = "requires the disc's music graphs"]
fn ipod_track_one_plays_segments_1_to_30_then_stops() {
    let g = graph("ipod.mpf");
    let mut s = Stream::new(&g, 1);
    assert!(s.set_var(&g, "ipod", 1));
    assert!(s.post(&g, 0x0416_D750));
    let walk: Vec<u32> = std::iter::from_fn(|| s.next_segment(&g)).take(100).collect();
    assert_eq!(walk, (0..30).collect::<Vec<_>>(), "bank segments 0..29 = subsongs 1..30");
    assert_eq!(s.callbacks, [(100, 1)]);
    // Track 2 starts at its own entry.
    let mut s = Stream::new(&g, 1);
    s.set_var(&g, "ipod", 2);
    s.post(&g, 0x16D750);
    assert_eq!(s.next_segment(&g), Some(30));
}

#[test]
#[ignore = "requires the disc's music graphs"]
fn chase_and_race_starts_pick_their_sections_for_every_seed() {
    let g = graph("game.mpf");
    let (chase, race) = (g.var("chasesection").unwrap(), g.var("racesection").unwrap());
    for seed in 1..200u32 {
        let mut s = Stream::new(&g, seed * 7919);
        assert!(s.post(&g, 0x1335A0));
        assert!((1..=4).contains(&s.vars[chase]), "chase section {} (seed {seed})", s.vars[chase]);
        let first = s.next_segment(&g).expect("chase music plays");
        let node = s.node.unwrap();
        assert!(matches!(g.nodes[node as usize].kind, Kind::Audio(_)) && g.nodes[node as usize].section == s.vars[chase] as u8, "seed {seed}: segment {first}");
        let mut s = Stream::new(&g, seed * 104_729);
        assert!(s.post(&g, 0x3BE42D));
        assert!((5..=12).contains(&s.vars[race]), "race section {} (seed {seed})", s.vars[race]);
        assert_eq!(s.control, 76);
        assert!(s.next_segment(&g).is_some(), "race music plays (seed {seed})");
    }
}
