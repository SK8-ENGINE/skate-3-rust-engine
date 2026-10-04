//! Integration test: two Rust clients speak the session protocol to a real Go
//! server and one receives the other's relayed body snapshot. This is the
//! closest automated equivalent of "open two games and see each other".
//!
//! The Go server binary is expected at `../server-go/bin/skated.exe` relative
//! to the workspace root. If it is absent the test is skipped, so `cargo test`
//! stays green on machines without the Go build.
use prost::Message;
use skate_proto::{v1, PROTOCOL_VERSION};
use std::net::UdpSocket;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Client {
    socket: UdpSocket,
    addr: std::net::SocketAddr,
    session_id: u64,
    actor_id: u64,
}

impl Client {
    fn connect(target: std::net::SocketAddr, id: u64, name: &str) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let hello = v1::Envelope {
            protocol_version: PROTOCOL_VERSION,
            session_id: 0,
            actor_id: 0,
            nonce: 1,
            message: Some(v1::envelope::Message::Hello(v1::Hello {
                protocol_version: PROTOCOL_VERSION,
                build: "rust-test".into(),
                lobby_id: 0,
                info: Some(v1::ClientInfo {
                    id,
                    map: 1,
                    rig: 1,
                    physics: 1,
                    appearance: 1,
                }),
                display_name: name.into(),
            })),
        };
        send(&socket, target, &hello);
        let mut client = Self {
            socket,
            addr: target,
            session_id: 0,
            actor_id: 0,
        };
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline && client.session_id == 0 {
            if let Some(envelope) = client.recv() {
                if let Some(v1::envelope::Message::Welcome(w)) = envelope.message {
                    client.session_id = w.session_id;
                    client.actor_id = w.actor_id;
                }
            }
        }
        assert!(client.session_id != 0, "client {name} got no welcome");
        client
    }

    fn recv(&self) -> Option<v1::Envelope> {
        let mut buffer = [0u8; 4096];
        match self.socket.recv_from(&mut buffer) {
            Ok((n, _)) => v1::Envelope::decode(&buffer[..n]).ok(),
            Err(_) => None,
        }
    }

    /// Polls until a relayed body snapshot from `origin` arrives, or times out.
    fn wait_for_body_from(&self, origin: u64, timeout: Duration) -> Option<v1::Snapshot> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(envelope) = self.recv() {
                if let Some(v1::envelope::Message::Snapshot(s)) = envelope.message {
                    let is_body = matches!(s.payload, Some(v1::snapshot::Payload::Body(_)));
                    if is_body && s.origin_actor == origin {
                        return Some(s);
                    }
                }
            }
        }
        None
    }

    fn send_body(&self, seq: u32, x: f32) {
        let snapshot = v1::Snapshot {
            kind: v1::StreamKind::Body as i32,
            origin_actor: self.actor_id,
            seq,
            payload: Some(v1::snapshot::Payload::Body(v1::BodySnapshot {
                captured_ms: 1,
                root: Some(v1::Pose {
                    px: x,
                    qw: 1.,
                    ..Default::default()
                }),
                enabled: 1,
                bodies: vec![],
            })),
        };
        let envelope = v1::Envelope {
            protocol_version: PROTOCOL_VERSION,
            session_id: self.session_id,
            actor_id: self.actor_id,
            nonce: seq,
            message: Some(v1::envelope::Message::Snapshot(snapshot)),
        };
        send(&self.socket, self.addr, &envelope);
    }
}

fn send(socket: &UdpSocket, addr: std::net::SocketAddr, envelope: &v1::Envelope) {
    let mut bytes = Vec::new();
    envelope.encode(&mut bytes).unwrap();
    socket.send_to(&bytes, addr).unwrap();
}

/// Spawns the Go server on a fixed loopback port. Returns None if missing.
fn start_server(port: u16) -> Option<Child> {
    let exe = "../server-go/bin/skated.exe";
    if !std::path::Path::new(exe).is_file() {
        eprintln!("skipping: {exe} not built");
        return None;
    }
    let child = Command::new(exe)
        .args(["-bind", &format!("127.0.0.1:{port}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn skated");
    // Give the socket time to bind before the first handshake.
    std::thread::sleep(Duration::from_millis(800));
    Some(child)
}

#[test]
fn two_clients_join_and_relay_a_body_snapshot() {
    const PORT: u16 = 31077;
    let Some(mut server) = start_server(PORT) else {
        return;
    };
    let target: std::net::SocketAddr = format!("127.0.0.1:{PORT}").parse().unwrap();

    let a = Client::connect(target, 9001, "Alpha");
    let b = Client::connect(target, 9002, "Bravo");
    assert_ne!(a.actor_id, b.actor_id, "server must assign distinct actors");

    // A publishes a body; B must receive it relayed with A's server actor id.
    a.send_body(1, 12.5);
    let relayed = b
        .wait_for_body_from(a.actor_id, Duration::from_secs(3))
        .expect("B did not receive A's relayed body snapshot");

    let Some(v1::snapshot::Payload::Body(body)) = relayed.payload else {
        panic!("relayed snapshot was not a body");
    };
    let root = body.root.expect("root pose");
    assert!(
        (root.px - 12.5).abs() < 1e-6,
        "relayed position should match, got {}",
        root.px
    );
    // The source capture time is the renderer's interpolation timeline. If the
    // server or client drops it, remote skaters freeze on the first sample.
    assert_eq!(
        body.captured_ms, 1,
        "captured_ms must be forwarded verbatim, not zeroed"
    );

    // The sender must not receive its own snapshot back.
    assert!(
        a.wait_for_body_from(a.actor_id, Duration::from_millis(600))
            .is_none(),
        "server echoed A's snapshot back to A"
    );

    server.kill().ok();
    server.wait().ok();
}
