//! Bounded appearance transfer over the lobby's authenticated, budgeted record stream.
//! Fixed keys keep late joins/retries reliable without growing lobby state per file.
use skate_net::lobby::Session;
use std::collections::BTreeMap;
const META: &str = "@look/meta";
const CHUNK: usize = 980;
const WINDOW: usize = 32;
pub const MAX_BLOB: usize = 64 * 1024 * 1024;
const SESSION_BUDGET: usize = 256 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub hash: [u8; 32],
    pub size: usize,
}
impl Identity {
    fn read(b: &[u8]) -> Option<Self> {
        if b.len() != 36 {
            return None;
        }
        let size = u32::from_le_bytes(b[32..].try_into().ok()?) as usize;
        (size > 0 && size <= MAX_BLOB).then(|| Self {
            hash: b[..32].try_into().unwrap(),
            size,
        })
    }
    fn bytes(&self) -> Vec<u8> {
        let mut b = self.hash.to_vec();
        b.extend((self.size as u32).to_le_bytes());
        b
    }
}
struct Incoming {
    id: Identity,
    bytes: Vec<u8>,
    received: Vec<bool>,
}
#[derive(Default)]
pub struct Exchange {
    local: Option<(Identity, Vec<u8>)>,
    incoming: BTreeMap<u64, Incoming>,
    pub ready: BTreeMap<u64, (Identity, Vec<u8>)>,
    spent: usize,
    known: BTreeMap<[u8; 32], Identity>,
    round: usize,
    next: u64,
    serving: Option<([u8; 32], usize, u64)>,
}
impl Exchange {
    pub fn remember(&mut self, id: Identity) {
        self.known.insert(id.hash, id);
    }
    pub fn progress(&self) -> String {
        let total: usize = self.incoming.values().map(|p| p.received.len()).sum();
        let done: usize = self
            .incoming
            .values()
            .map(|p| p.received.iter().filter(|v| **v).count())
            .sum();
        if total == 0 {
            format!("Characters: {} synced", self.ready.len())
        } else {
            format!(
                "Characters: {} synced | downloading {} ({}%)",
                self.ready.len(),
                self.incoming.len(),
                done * 100 / total
            )
        }
    }
    pub fn publish(&mut self, bytes: Vec<u8>) -> bool {
        if bytes.is_empty() || bytes.len() > MAX_BLOB {
            return false;
        }
        let id = Identity {
            hash: *blake3::hash(&bytes).as_bytes(),
            size: bytes.len(),
        };
        self.local = Some((id, bytes));
        true
    }
    pub fn tick(&mut self, session: &mut Session, now: u64) {
        if now < self.next {
            return;
        }
        self.next = now + 25;
        self.incoming
            .retain(|id, _| session.actors.contains_key(id));
        self.ready.retain(|id, _| session.actors.contains_key(id));
        let Some((local, bytes)) = &self.local else {
            return;
        };
        session.publish_application(META, local.bytes(), now);
        let mut records = vec![];
        let mut requests = vec![];
        for (slot, (&actor, a)) in session.actors.iter().enumerate() {
            if actor == session.local {
                continue;
            }
            for r in a
                .application
                .iter()
                .filter(|(key, _)| key.starts_with("@look/r/"))
                .map(|(_, r)| r)
            {
                if r.value.len() == 44
                    && u64::from_le_bytes(r.value[..8].try_into().unwrap()) == session.local
                    && r.value[8..40] == local.hash
                {
                    let page = u32::from_le_bytes(r.value[40..].try_into().unwrap()) as usize;
                    if page < bytes.len().div_ceil(CHUNK * WINDOW) {
                        requests.push(page);
                    }
                }
            }
            let Some(id) = a
                .application
                .get(META)
                .and_then(|r| Identity::read(&r.value))
            else {
                continue;
            };
            if self.ready.get(&actor).is_some_and(|(old, _)| old == &id) {
                self.incoming.remove(&actor);
                continue;
            }
            if self.known.get(&id.hash) == Some(&id) {
                self.incoming.remove(&actor);
                self.ready.insert(actor, (id, vec![]));
                continue;
            }
            if self.incoming.get(&actor).is_none_or(|p| p.id != id) {
                if self.spent.saturating_add(id.size) > SESSION_BUDGET {
                    continue;
                }
                self.spent += id.size;
                self.incoming.insert(
                    actor,
                    Incoming {
                        received: vec![false; id.size.div_ceil(CHUNK)],
                        bytes: vec![0; id.size],
                        id,
                    },
                );
            }
            let p = self.incoming.get_mut(&actor).unwrap();
            for i in 0..WINDOW {
                let Some(r) = a.application.get(&format!("@look/c/{i}")) else {
                    continue;
                };
                let b = &r.value;
                if b.len() < 37 || b[..32] != p.id.hash {
                    continue;
                }
                let offset = u32::from_le_bytes(b[32..36].try_into().unwrap()) as usize;
                if offset % CHUNK != 0
                    || offset >= p.bytes.len()
                    || b.len() - 36 != CHUNK.min(p.bytes.len() - offset)
                {
                    continue;
                }
                p.bytes[offset..offset + b.len() - 36].copy_from_slice(&b[36..]);
                p.received[offset / CHUNK] = true;
            }
            if let Some(missing) = p.received.iter().position(|done| !*done) {
                let mut request = actor.to_le_bytes().to_vec();
                request.extend(p.id.hash);
                request.extend(((missing / WINDOW) as u32).to_le_bytes());
                records.push((format!("@look/r/{slot}"), request));
            } else {
                let p = self.incoming.remove(&actor).unwrap();
                if blake3::hash(&p.bytes).as_bytes() == &p.id.hash {
                    self.ready.insert(actor, (p.id, p.bytes));
                }
                // Clear the request after completion, including corrupted transfers.
                records.push((format!("@look/r/{slot}"), vec![]));
            }
        }
        // Requests occupy stable bounded slots, but actor ordering can change.
        // Explicit tombstones stop transfers after completion/cache hits/departures.
        for slot in 0..skate_net::lobby::MAX_PLAYERS {
            let key = format!("@look/r/{slot}");
            if !records.iter().any(|(k, _)| k == &key) {
                records.push((key, vec![]));
            }
        }
        if requests.is_empty() {
            for slot in 0..WINDOW {
                records.push((format!("@look/c/{slot}"), vec![]));
            }
        }
        if !requests.is_empty() {
            requests.sort_unstable();
            requests.dedup();
            let page = if let Some((hash, page, at)) = self.serving.filter(|(hash, page, at)| {
                *hash == local.hash && requests.contains(page) && now.saturating_sub(*at) < 150
            }) {
                let _ = (hash, at);
                page
            } else {
                let page = requests[self.round % requests.len()];
                self.round = self.round.wrapping_add(1);
                self.serving = Some((local.hash, page, now));
                page
            };
            for slot in 0..WINDOW {
                let offset = (page * WINDOW + slot) * CHUNK;
                if offset >= bytes.len() {
                    break;
                }
                let mut b = local.hash.to_vec();
                b.extend((offset as u32).to_le_bytes());
                b.extend(&bytes[offset..(offset + CHUNK).min(bytes.len())]);
                records.push((format!("@look/c/{slot}"), b));
            }
        }
        for (key, value) in records {
            session.publish_application(&key, value, now);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use skate_net::lobby::Info;
    #[test]
    fn online_appearance_transfer_survives_loss_reordering_late_join_and_changes() {
        let mut sessions: Vec<_> = (0..10)
            .map(|i| {
                Session::new(
                    42,
                    Info {
                        id: 100 + i,
                        map: 1,
                        rig: 2,
                        physics: 3,
                        appearance: 4,
                    },
                    if i == 0 { None } else { Some(1) },
                )
            })
            .collect();
        let mut exchanges: Vec<_> = (0..10).map(|_| Exchange::default()).collect();
        let mut expected: Vec<_> = (0..10)
            .map(|i| {
                (0..(90000 + i * 1000))
                    .map(|j| (j * 17 + i) as u8)
                    .collect::<Vec<_>>()
            })
            .collect();
        for (e, b) in exchanges.iter_mut().zip(&expected) {
            assert!(e.publish(b.clone()));
        }
        let mut completed = false;
        for step in 0..16000 {
            let now = step * 25;
            let active = if step < 200 { 3 } else { 10 };
            if step == 500 {
                expected[1] = vec![77; 123456];
                exchanges[1].publish(expected[1].clone());
            }
            let mut wire = vec![];
            for i in 0..active {
                if step % 4 == 0 {
                    let pose = skate_net::Pose {
                        p: [step as f32 * 0.001, i as f32, 0.],
                        q: [0., 0., 0., 1.],
                    };
                    let body = skate_net::Body {
                        pose,
                        velocity: [0.; 3],
                        angular: [0.; 3],
                    };
                    let mut packed =
                        skate_net::packed::Packed::body(&skate_net::packed::BodyState {
                            root: pose,
                            enabled: (1u64 << 33) - 1,
                            bodies: vec![body; 33],
                        })
                        .unwrap();
                    packed.captured = now;
                    sessions[i].publish(skate_net::packed::BODY, packed, now);
                }
                exchanges[i].tick(&mut sessions[i], now);
                for p in sessions[i].service(now) {
                    if (step + i as u64 + p.data.len() as u64) % 7 != 0 {
                        wire.push((i + 1, p));
                    }
                }
            }
            wire.reverse();
            for (from, p) in wire {
                let to = p.peer as usize - 1;
                if to < active {
                    sessions[to].receive(from as u64, &p.data, now);
                    if step % 13 == 0 {
                        sessions[to].receive(from as u64, &p.data, now);
                    }
                }
            }
            if step > 500
                && exchanges.iter().enumerate().all(|(i, e)| {
                    expected.iter().enumerate().all(|(j, b)| {
                        i == j || e.ready.get(&(100 + j as u64)).is_some_and(|(_, v)| v == b)
                    })
                })
            {
                for session in &sessions {
                    for (&id, actor) in &session.actors {
                        if id != session.local {
                            assert!(
                                actor.body.latest().is_some_and(|b| now - b.received < 1000),
                                "appearance transfer must not starve movement"
                            );
                        }
                    }
                }
                completed = true;
                break;
            }
        }
        assert!(
            completed,
            "every player must receive the latest complete appearance"
        );
        for session in &sessions {
            for actor in session.actors.values() {
                assert!(actor.application.len() <= 43);
            }
        }
    }
    #[test]
    fn online_appearance_rejects_unbounded_metadata_and_chunks() {
        assert!(Identity::read(&[]).is_none());
        let id = Identity {
            hash: [1; 32],
            size: MAX_BLOB + 1,
        };
        assert!(Identity::read(&id.bytes()).is_none());
        let mut e = Exchange::default();
        assert!(!e.publish(vec![]));
    }
}
#[cfg(test)]
mod delivery_tests {
    use super::*;
    #[test]
    fn online_appearance_large_model_delivered_once_cached_and_quiet_after_swap_back() {
        let mut sessions: Vec<_> = (0..2)
            .map(|i| {
                Session::new(
                    44,
                    skate_net::lobby::Info {
                        id: 10 + i,
                        map: 1,
                        rig: 1,
                        physics: 1,
                        appearance: 1,
                    },
                    if i == 0 { None } else { Some(1) },
                )
            })
            .collect();
        for s in &mut sessions {
            s.set_loopback(true);
        }
        let mut exchanges = [Exchange::default(), Exchange::default()];
        let model: Vec<u8> = (0..32_067_793).map(|i| (i * 17) as u8).collect();
        exchanges[0].publish(model.clone());
        exchanges[1].publish(vec![4]);
        let mut phase = 0;
        let mut finished_at = 0;
        let mut last_model_packet = 0;
        let mut reused = false;
        for step in 0..18000 {
            let now = step * 10;
            let mut wire = vec![];
            for i in 0..2 {
                exchanges[i].tick(&mut sessions[i], now);
                for p in sessions[i].service(now) {
                    if p.data.get(24) == Some(&skate_net::lobby::APPLICATION) && p.data.len() > 100
                    {
                        last_model_packet = now;
                        assert!(phase < 3, "completed model was retransmitted");
                    }
                    if step % 31 != 0 {
                        wire.push((i + 1, p));
                    }
                }
            }
            for (from, p) in wire {
                sessions[p.peer as usize - 1].receive(from as u64, &p.data, now);
            }
            if phase == 0
                && exchanges[1]
                    .ready
                    .get(&10)
                    .is_some_and(|(_, b)| b == &model)
            {
                assert!(now < 90000, "local model transfer is too slow: {now} ms");
                let identity = exchanges[1].ready[&10].0.clone();
                exchanges[1].remember(identity);
                exchanges[1].ready.get_mut(&10).unwrap().1.clear();
                exchanges[0].publish(vec![9]);
                phase = 1;
            }
            if phase == 1
                && exchanges[1]
                    .ready
                    .get(&10)
                    .is_some_and(|(_, b)| b == &vec![9])
            {
                exchanges[0].publish(model.clone());
                phase = 2;
                finished_at = now;
            }
            if phase == 2
                && exchanges[1]
                    .ready
                    .get(&10)
                    .is_some_and(|(id, b)| id.size == model.len() && b.is_empty())
            {
                reused = true;
                if now - finished_at > 2000 {
                    phase = 3;
                    finished_at = now;
                }
            }
            if phase == 3 && now - finished_at > 5000 {
                assert!(last_model_packet < finished_at);
                break;
            }
        }
        assert_eq!(phase, 3);
        assert!(reused);
    }
}
