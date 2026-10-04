//! Authoritative session client: speaks the Protobuf `skated` protocol over a
//! single UDP socket. The server is the hub — every snapshot this client sends
//! goes to the server, and the server relays peers' snapshots back. The client
//! never needs peer addresses.
//!
//! This module is deliberately transport-only: it converts between the engine's
//! `Pose`/`Body`/`Bone` types and the generated prost types, and exposes a
//! simple `send`/`poll` surface. It does not touch Bevy.
use prost::Message;
use skate_net::{Body, Bone, Pose};
use skate_proto::v1;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

/// Fixed snapshot rates in the server mode. BODY carries the 33 rigid bodies,
/// POSE the animation anchors; both are full snapshots (no delta).
const BODY_INTERVAL: Duration = Duration::from_millis(50);
const POSE_INTERVAL: Duration = Duration::from_millis(100);
/// Handshake retry cadence before Welcome arrives.
const HELLO_RETRY: Duration = Duration::from_millis(500);
/// Liveness ping cadence once connected.
const PING_INTERVAL: Duration = Duration::from_millis(500);
/// A hello that never gets a welcome times out.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// A remote member as advertised by the server roster.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PeerInfo {
    pub actor: u64,
    pub rig: u64,
    pub display_name: String,
}

/// What the session produced this frame.
#[derive(Default)]
pub(crate) struct Polled {
    /// Decoded body snapshots received this frame, keyed by origin actor.
    pub bodies: Vec<(u64, u32, u64, skate_net::packed::BodyState)>,
    /// Decoded pose snapshots received this frame, keyed by origin actor.
    pub poses: Vec<(u64, u32, u64, skate_net::packed::PoseState)>,
    /// Application records (names, mod state) received this frame.
    pub applications: Vec<(u64, String, u32, Vec<u8>)>,
}

/// Client state for one connection to the server.
pub(crate) struct SkatedSession {
    socket: UdpSocket,
    /// Server address, resolved once.
    server: std::net::SocketAddr,

    session_id: u64,
    actor_id: u64,
    lobby_id: u64,
    epoch: u64,
    connected: bool,
    notice: String,

    started: Instant,
    last_hello: Instant,
    last_ping: Instant,
    last_body: Instant,
    last_pose: Instant,
    nonce: u32,

    body_seq: u32,
    pose_seq: u32,
    app_seq: u32,

    pub peers: std::collections::BTreeMap<u64, PeerInfo>,
    /// Latest application records seen per (actor, key), for names and mod state.
    pub applications: std::collections::BTreeMap<(u64, String), (u32, Vec<u8>)>,
    /// Peers joined since the last time the game drained this flag.
    pub roster_changed: bool,
    pub rtt_ms: u64,
}

impl SkatedSession {
    /// Binds a local socket and starts the handshake with `server`.
    pub fn connect(
        server: std::net::SocketAddr,
        info: v1::ClientInfo,
        display_name: String,
    ) -> std::io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        skate_net::socket::configure(&socket)?;
        let now = Instant::now();
        let mut session = Self {
            socket,
            server,
            session_id: 0,
            actor_id: 0,
            lobby_id: 0,
            epoch: 0,
            connected: false,
            notice: "Connecting to session server...".into(),
            started: now,
            last_hello: now - HELLO_RETRY,
            last_ping: now,
            last_body: now,
            last_pose: now,
            nonce: 0,
            body_seq: 0,
            pose_seq: 0,
            app_seq: 0,
            peers: std::collections::BTreeMap::new(),
            applications: std::collections::BTreeMap::new(),
            roster_changed: false,
            rtt_ms: 0,
        };
        session.send_hello(&info, &display_name);
        Ok(session)
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }
    pub fn actor_id(&self) -> u64 {
        self.actor_id
    }
    pub fn lobby_id(&self) -> u64 {
        self.lobby_id
    }
    pub fn notice(&self) -> &str {
        &self.notice
    }
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    fn next_nonce(&mut self) -> u32 {
        self.nonce = self.nonce.wrapping_add(1);
        self.nonce
    }

    fn send_hello(&mut self, info: &v1::ClientInfo, display_name: &str) {
        let envelope = v1::Envelope {
            protocol_version: skate_proto::PROTOCOL_VERSION,
            session_id: 0,
            actor_id: 0,
            nonce: self.next_nonce(),
            message: Some(v1::envelope::Message::Hello(v1::Hello {
                protocol_version: skate_proto::PROTOCOL_VERSION,
                build: "skate3rust".into(),
                lobby_id: 0,
                info: Some(info.clone()),
                display_name: display_name.to_owned(),
            })),
        };
        self.raw_send(&envelope);
        self.last_hello = Instant::now();
    }

    fn raw_send(&self, envelope: &v1::Envelope) {
        let mut bytes = Vec::with_capacity(128);
        if envelope.encode(&mut bytes).is_ok() {
            let _ = self.socket.send_to(&bytes, self.server);
        }
    }

    fn send_envelope(&mut self, message: v1::envelope::Message) {
        let envelope = v1::Envelope {
            protocol_version: skate_proto::PROTOCOL_VERSION,
            session_id: self.session_id,
            actor_id: self.actor_id,
            nonce: self.next_nonce(),
            message: Some(message),
        };
        self.raw_send(&envelope);
    }

    /// Services the socket: retries the handshake, keeps liveness, and decodes
    /// any incoming envelopes. Call once per frame.
    pub fn poll(&mut self, info: &v1::ClientInfo, display_name: &str) -> Polled {
        self.retry_handshake(info, display_name);
        self.keepalive();

        let mut out = Polled::default();
        // A relayed BODY snapshot (33 bodies with full poses and rates) can be
        // several KiB. A small buffer makes Windows fail the read with
        // WSAEMSGSIZE and the datagram is lost, so size it above any snapshot.
        let mut buffer = [0u8; 64 * 1024];
        for _ in 0..256 {
            match self.socket.recv_from(&mut buffer) {
                Ok((n, from)) => {
                    if from != self.server {
                        continue;
                    }
                    if let Ok(envelope) = v1::Envelope::decode(&buffer[..n]) {
                        self.handle(envelope, &mut out);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(_) => break,
            }
        }
        out
    }

    fn retry_handshake(&mut self, info: &v1::ClientInfo, display_name: &str) {
        if self.connected {
            return;
        }
        if self.started.elapsed() > HANDSHAKE_TIMEOUT {
            self.notice = "Session server did not respond. Check the address and firewall.".into();
        }
        if self.last_hello.elapsed() >= HELLO_RETRY {
            self.send_hello(info, display_name);
        }
    }

    fn keepalive(&mut self) {
        if !self.connected || self.last_ping.elapsed() < PING_INTERVAL {
            return;
        }
        self.send_ping();
    }

    fn send_ping(&mut self) {
        let nonce = u64::from(self.next_nonce());
        let client_time_ms = self.started.elapsed().as_millis() as u64;
        self.send_envelope(v1::envelope::Message::Ping(v1::Ping {
            nonce,
            client_time_ms,
        }));
        self.last_ping = Instant::now();
    }

    fn handle(&mut self, envelope: v1::Envelope, out: &mut Polled) {
        match envelope.message {
            Some(v1::envelope::Message::Reject(reject)) => {
                self.notice = format!("Session rejected: {}", reject.detail);
            }
            Some(v1::envelope::Message::Welcome(welcome)) => {
                self.session_id = welcome.session_id;
                self.actor_id = welcome.actor_id;
                self.lobby_id = welcome.lobby_id;
                self.epoch = welcome.epoch;
                self.connected = true;
                self.notice.clear();
                self.apply_roster(welcome.roster);
                // Announce liveness immediately: the game may block for seconds
                // loading assets before the next frame runs poll(), and the
                // server must not time the session out in that gap.
                self.send_ping();
            }
            Some(v1::envelope::Message::Roster(roster)) => {
                if roster.epoch >= self.epoch {
                    self.epoch = roster.epoch;
                    self.apply_roster(roster.entries);
                }
            }
            Some(v1::envelope::Message::Pong(pong)) => {
                let now = self.started.elapsed().as_millis() as u64;
                if pong.client_time_ms <= now {
                    self.rtt_ms = now - pong.client_time_ms;
                }
            }
            Some(v1::envelope::Message::Snapshot(snapshot)) => {
                self.apply_snapshot(snapshot, out);
            }
            _ => {}
        }
    }

    fn apply_roster(&mut self, entries: Vec<v1::RosterEntry>) {
        let mut next = std::collections::BTreeMap::new();
        for entry in entries {
            if entry.actor_id == self.actor_id {
                continue;
            }
            let rig = entry.info.as_ref().map_or(0, |i| i.rig);
            next.insert(
                entry.actor_id,
                PeerInfo {
                    actor: entry.actor_id,
                    rig,
                    display_name: entry.display_name,
                },
            );
        }
        self.roster_changed = next != self.peers;
        self.peers = next;
    }

    fn apply_snapshot(&mut self, snapshot: v1::Snapshot, out: &mut Polled) {
        match snapshot.payload {
            Some(v1::snapshot::Payload::Body(body)) => {
                if let Some(state) = unpack_body(&body) {
                    out.bodies
                        .push((snapshot.origin_actor, snapshot.seq, body.captured_ms, state));
                }
            }
            Some(v1::snapshot::Payload::Pose(pose)) => {
                if let Some(state) = unpack_pose(&pose) {
                    out.poses
                        .push((snapshot.origin_actor, snapshot.seq, pose.captured_ms, state));
                }
            }
            Some(v1::snapshot::Payload::Application(record)) => {
                let entry = self
                    .applications
                    .entry((snapshot.origin_actor, record.key.clone()))
                    .or_insert((0, Vec::new()));
                if record.seq >= entry.0 {
                    *entry = (record.seq, record.value.clone());
                }
                out.applications.push((
                    snapshot.origin_actor,
                    record.key,
                    record.seq,
                    record.value,
                ));
            }
            None => {}
        }
    }

    /// Publishes a body snapshot if the fixed interval elapsed. Returns true
    /// when a datagram was sent.
    pub fn maybe_send_body(
        &mut self,
        state: &skate_net::packed::BodyState,
        captured_ms: u64,
    ) -> bool {
        if !self.connected || self.last_body.elapsed() < BODY_INTERVAL {
            return false;
        }
        self.body_seq = self.body_seq.wrapping_add(1);
        let snapshot = v1::Snapshot {
            kind: v1::StreamKind::Body as i32,
            origin_actor: self.actor_id,
            seq: self.body_seq,
            payload: Some(v1::snapshot::Payload::Body(pack_body(state, captured_ms))),
        };
        self.send_envelope(v1::envelope::Message::Snapshot(snapshot));
        self.last_body = Instant::now();
        true
    }

    /// Publishes a pose snapshot on its own interval.
    pub fn maybe_send_pose(
        &mut self,
        state: &skate_net::packed::PoseState,
        captured_ms: u64,
    ) -> bool {
        if !self.connected || self.last_pose.elapsed() < POSE_INTERVAL {
            return false;
        }
        self.pose_seq = self.pose_seq.wrapping_add(1);
        let snapshot = v1::Snapshot {
            kind: v1::StreamKind::Pose as i32,
            origin_actor: self.actor_id,
            seq: self.pose_seq,
            payload: Some(v1::snapshot::Payload::Pose(pack_pose_state(
                state,
                captured_ms,
            ))),
        };
        self.send_envelope(v1::envelope::Message::Snapshot(snapshot));
        self.last_pose = Instant::now();
        true
    }

    /// Publishes a key/value application record (name, mod state). The server
    /// relays it to other members; repeated sends are idempotent on the client.
    pub fn publish_application(&mut self, key: &str, value: Vec<u8>) {
        if !self.connected {
            return;
        }
        self.app_seq = self.app_seq.wrapping_add(1);
        let snapshot = v1::Snapshot {
            kind: v1::StreamKind::Application as i32,
            origin_actor: self.actor_id,
            seq: self.app_seq,
            payload: Some(v1::snapshot::Payload::Application(v1::ApplicationRecord {
                key: key.to_owned(),
                seq: self.app_seq,
                value,
            })),
        };
        self.send_envelope(v1::envelope::Message::Snapshot(snapshot));
    }

    /// Sends a polite goodbye so the server drops this member immediately.
    pub fn goodbye(&mut self) {
        if self.connected {
            self.send_envelope(v1::envelope::Message::Goodbye(v1::Goodbye {
                detail: "client leaving".into(),
            }));
        }
    }
}

// --- Codec between engine types and prost types ------------------------------

fn pack_pose(pose: Pose) -> v1::Pose {
    v1::Pose {
        px: pose.p[0],
        py: pose.p[1],
        pz: pose.p[2],
        qx: pose.q[0],
        qy: pose.q[1],
        qz: pose.q[2],
        qw: pose.q[3],
    }
}

fn unpack_pose_message(pose: &v1::Pose) -> Pose {
    Pose {
        p: [pose.px, pose.py, pose.pz],
        q: [pose.qx, pose.qy, pose.qz, pose.qw],
    }
}

/// Lenient pose decoding for relayed snapshots.
///
/// The owner of a skater is authoritative for its own simulation, and the
/// solver can hold quaternions that are physically valid but not unit-length
/// to within `Pose::valid`'s strict tolerance. A relayed snapshot must be
/// accepted and normalised rather than discarded wholesale, otherwise one
/// slightly-off body would drop the entire 33-body update.
fn unpack_pose_lenient(pose: &v1::Pose) -> Option<Pose> {
    let raw = unpack_pose_message(pose);
    if !raw.p.iter().all(|v| v.is_finite() && v.abs() < 100_000.) {
        return None;
    }
    if !raw.q.iter().all(|v| v.is_finite()) {
        return None;
    }
    let norm = raw.q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !(norm > 1e-6) {
        return None;
    }
    Some(Pose {
        p: raw.p,
        q: raw.q.map(|v| v / norm),
    })
}

fn pack_body(state: &skate_net::packed::BodyState, captured_ms: u64) -> v1::BodySnapshot {
    v1::BodySnapshot {
        captured_ms,
        root: Some(pack_pose(state.root)),
        enabled: state.enabled,
        bodies: state
            .bodies
            .iter()
            .map(|body| v1::Body {
                pose: Some(pack_pose(body.pose)),
                vx: body.velocity[0],
                vy: body.velocity[1],
                vz: body.velocity[2],
                wx: body.angular[0],
                wy: body.angular[1],
                wz: body.angular[2],
            })
            .collect(),
    }
}

fn unpack_body(message: &v1::BodySnapshot) -> Option<skate_net::packed::BodyState> {
    let root = message.root.as_ref().and_then(unpack_pose_lenient)?;
    let bodies = message
        .bodies
        .iter()
        .map(|body| {
            let pose = body.pose.as_ref().and_then(unpack_pose_lenient)?;
            Some(Body {
                pose,
                velocity: [body.vx, body.vy, body.vz],
                angular: [body.wx, body.wy, body.wz],
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(skate_net::packed::BodyState {
        root,
        enabled: message.enabled,
        bodies,
    })
}

fn pack_pose_state(state: &skate_net::packed::PoseState, captured_ms: u64) -> v1::PoseSnapshot {
    v1::PoseSnapshot {
        captured_ms,
        root: Some(pack_pose(state.root)),
        bones: state
            .bones
            .iter()
            .map(|bone| v1::Bone {
                index: u32::from(bone.index),
                pose: Some(pack_pose(bone.pose)),
            })
            .collect(),
    }
}

fn unpack_pose(message: &v1::PoseSnapshot) -> Option<skate_net::packed::PoseState> {
    let root = message.root.as_ref().and_then(unpack_pose_lenient)?;
    let mut bones = Vec::with_capacity(message.bones.len());
    for bone in &message.bones {
        if bone.index >= 256 {
            return None;
        }
        bones.push(Bone {
            index: bone.index as u16,
            pose: bone.pose.as_ref().and_then(unpack_pose_lenient)?,
        });
    }
    Some(skate_net::packed::PoseState { root, bones })
}
/// Builds a [`v1::ClientInfo`] from the engine's compatibility fingerprints.
pub(crate) fn client_info(
    id: u64,
    map: u64,
    rig: u64,
    physics: u64,
    appearance: u64,
) -> v1::ClientInfo {
    v1::ClientInfo {
        id,
        map,
        rig,
        physics,
        appearance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_net::packed::{BodyState, PoseState};
    use skate_net::{Body, Bone, Pose};

    fn pose(x: f32) -> Pose {
        Pose {
            p: [x, 1., 2.],
            q: [0., 0., 0., 1.],
        }
    }

    fn body_state() -> BodyState {
        BodyState {
            root: pose(0.),
            enabled: (1u64 << 33) - 1,
            bodies: (0..33)
                .map(|i| Body {
                    pose: pose(i as f32),
                    velocity: [i as f32, 0., 1.],
                    angular: [0., i as f32, 0.],
                })
                .collect(),
        }
    }

    /// The engine's body snapshot survives the Protobuf round-trip unchanged.
    /// This is the core invariant for relayed movement.
    #[test]
    fn body_round_trips_through_protobuf() {
        let state = body_state();
        let packed = pack_body(&state, 1234);
        // The source capture time must survive verbatim: relaying it as zero
        // freezes remote interpolation on the receiver.
        assert_eq!(packed.captured_ms, 1234);
        // Encode/decode the whole payload, as the wire does.
        let mut bytes = Vec::new();
        packed.encode(&mut bytes).unwrap();
        let decoded = v1::BodySnapshot::decode(bytes.as_slice()).unwrap();
        let back = unpack_body(&decoded).expect("decode");
        assert_eq!(decoded.captured_ms, 1234);
        assert_eq!(back.root, state.root);
        assert_eq!(back.enabled, state.enabled);
        assert_eq!(back.bodies.len(), 33);
        for (original, restored) in state.bodies.iter().zip(&back.bodies) {
            assert_eq!(original.pose, restored.pose);
            assert_eq!(original.velocity, restored.velocity);
            assert_eq!(original.angular, restored.angular);
        }
    }

    /// Pose snapshots (animation anchors) likewise survive intact.
    #[test]
    fn pose_round_trips_through_protobuf() {
        let state = PoseState {
            root: pose(0.),
            bones: (0..8)
                .map(|i| Bone {
                    index: i,
                    pose: pose(i as f32),
                })
                .collect(),
        };
        let packed = pack_pose_state(&state, 4321);
        assert_eq!(packed.captured_ms, 4321);
        let mut bytes = Vec::new();
        packed.encode(&mut bytes).unwrap();
        let decoded = v1::PoseSnapshot::decode(bytes.as_slice()).unwrap();
        let back = unpack_pose(&decoded).expect("decode");
        assert_eq!(back.root, state.root);
        assert_eq!(back.bones.len(), 8);
        for (original, restored) in state.bones.iter().zip(&back.bones) {
            assert_eq!(original.index, restored.index);
            assert_eq!(original.pose, restored.pose);
        }
    }

    /// A non-normalised or out-of-range quaternion is rejected, not trusted.
    #[test]
    fn invalid_pose_is_rejected() {
        let mut message = pack_body(&body_state(), 0);
        message.root = Some(v1::Pose {
            qx: 0.,
            qy: 0.,
            qz: 0.,
            qw: 0.,
            ..Default::default()
        });
        assert!(unpack_body(&message).is_none(), "zero quaternion must fail");
    }
}
