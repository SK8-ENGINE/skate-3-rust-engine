//! Manual end-to-end client for the authoritative session server.
//!
//! It performs the same handshake a game client does (Hello -> Welcome -> Ping)
//! against a running `skated`, printing what it sees. Used to verify Go <-> Rust
//! interop without launching the full game.
//!
//! Run the server first:
//!     cd server-go && ./bin/skated.exe -bind 127.0.0.1:31030
//! Then:
//!     cargo run -p skate-proto --example session_smoke -- 127.0.0.1:31030
use prost::Message;
use skate_proto::{v1, PROTOCOL_VERSION};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

fn main() {
    let target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:31030".to_string());
    let addr: std::net::SocketAddr = target.parse().expect("address");

    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind client");
    socket
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();

    let hello = v1::Envelope {
        protocol_version: PROTOCOL_VERSION,
        session_id: 0,
        actor_id: 0,
        nonce: 1,
        message: Some(v1::envelope::Message::Hello(v1::Hello {
            protocol_version: PROTOCOL_VERSION,
            build: "rust-smoke".into(),
            lobby_id: 0,
            info: Some(v1::ClientInfo {
                id: 7,
                map: 1,
                rig: 1,
                physics: 1,
                appearance: 1,
            }),
            display_name: "RustSmoke".into(),
        })),
    };
    let mut bytes = Vec::new();
    hello.encode(&mut bytes).unwrap();
    socket.send_to(&bytes, addr).expect("send hello");
    println!("-> Hello sent to {addr}");

    let started = Instant::now();
    let mut buffer = [0u8; 2048];
    let mut welcome: Option<v1::Welcome> = None;
    while started.elapsed() < Duration::from_secs(4) {
        match socket.recv_from(&mut buffer) {
            Ok((n, _)) => {
                let Ok(envelope) = v1::Envelope::decode(&buffer[..n]) else {
                    continue;
                };
                match envelope.message {
                    Some(v1::envelope::Message::Welcome(w)) => {
                        println!(
                            "<- Welcome session={} actor={} lobby={} players={}",
                            w.session_id,
                            w.actor_id,
                            w.lobby_id,
                            w.roster.len()
                        );
                        // Send a ping and a body snapshot to exercise the relay path.
                        let ping = v1::Envelope {
                            protocol_version: PROTOCOL_VERSION,
                            session_id: w.session_id,
                            actor_id: w.actor_id,
                            nonce: 2,
                            message: Some(v1::envelope::Message::Ping(v1::Ping {
                                nonce: 42,
                                client_time_ms: 0,
                            })),
                        };
                        let mut out = Vec::new();
                        ping.encode(&mut out).unwrap();
                        socket.send_to(&out, addr).unwrap();
                        welcome = Some(w);
                    }
                    Some(v1::envelope::Message::Roster(r)) => {
                        println!("<- Roster epoch={} players={}", r.epoch, r.entries.len());
                    }
                    Some(v1::envelope::Message::Pong(p)) => {
                        println!("<- Pong nonce={}", p.nonce);
                    }
                    Some(v1::envelope::Message::Reject(r)) => {
                        eprintln!("<- Reject reason={}: {}", r.reason, r.detail);
                        std::process::exit(1);
                    }
                    _ => {}
                }
            }
            Err(_) => {}
        }
    }

    if welcome.is_some() {
        println!("OK: Rust client negotiated a session with the Go server");
    } else {
        eprintln!("FAIL: no welcome received");
        std::process::exit(1);
    }
}
