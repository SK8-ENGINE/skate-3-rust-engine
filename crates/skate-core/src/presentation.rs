//! The presentation runtime (retail PresentationManager; research b964 §2 R4-R11, b943, b900): groups of presentation
//! events (callouts, fades, waits, movies, speech ...) played one group at a time with retail's queue rules. Engine-free:
//! the game supplies the event handlers by type name ([`Handlers`]). Port step 2 of b964 §5; the handler registry and
//! the tutorial / career wiring are later steps.

use std::collections::VecDeque;

pub use crate::career::graph::Event;

/// A group's stable id: its owner plus a slot (`tut_sksc_08/intro`, `career/<graph>/<state>`, `mod:<id>/...`).
pub type GroupId = String;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    /// Who added it (a challenge, the career, a mod); `Manager::remove_owner` drops all of an owner's groups.
    pub owner: String,
    pub events: Vec<Event>,
}

/// What `Manager::start` did with a group (82DBACA0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Started {
    Now,
    Queued,
    /// Equal to the playing group with nothing queued, or to the last queued one.
    Skipped,
    /// The queue was full.
    Dropped,
}

/// What a frame of the manager produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    /// A group played to its end, or was skipped (`presentationeventgroupfinished` {group}).
    GroupFinished(GroupId),
    /// A group was removed with its owner (no finish message).
    GroupRemoved(GroupId),
}

/// The event handlers, by the event's type name (retail's handler map; R11). An instant event starts and is done at
/// once; unknown types should be logged and report done (retail's map miss is not read: ours).
pub trait Handlers {
    fn start(&mut self, event: &Event);
    fn update(&mut self, _event: &Event, _dt: f32) {}
    fn is_done(&mut self, event: &Event) -> bool;
    /// Undo an event that already ran (a skipped group, in reverse order).
    fn undo(&mut self, _event: &Event) {}
    /// Stop the event that is running (a skipped or removed group).
    fn stop(&mut self, _event: &Event) {}
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Running {
    group: Group,
    index: usize,
    started: bool,
}

/// The manager: one group plays, up to `capacity` wait (retail 8 slots).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manager {
    playing: Option<Running>,
    queue: VecDeque<Group>,
    pub capacity: usize,
    /// Events whose mask shares a bit with this are skipped (manager+0xC; 0 in shipped data, its writer is open).
    pub skip_mask: u32,
}

impl Default for Manager {
    fn default() -> Self {
        Self { playing: None, queue: VecDeque::new(), capacity: 8, skip_mask: 0 }
    }
}

impl Manager {
    /// The playing group's id.
    pub fn playing(&self) -> Option<&str> {
        self.playing.as_ref().map(|r| r.group.id.as_str())
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// AddPresentationGroup + StartPresentation (82DBACA0): start now when idle, else queue (R5).
    pub fn start(&mut self, group: Group) -> Started {
        let Some(playing) = &self.playing else {
            self.playing = Some(Running { group, index: 0, started: false });
            return Started::Now;
        };
        let same_as_last = match self.queue.back() {
            Some(last) => last.id == group.id,
            None => playing.group.id == group.id,
        };
        if same_as_last {
            Started::Skipped
        } else if self.queue.len() >= self.capacity {
            Started::Dropped
        } else {
            self.queue.push_back(group);
            Started::Queued
        }
    }

    /// One frame (82DBADE8): the playing group's events in order, each started, updated and polled until done; a masked
    /// event is skipped; at the group's end the finish message, then the next queued group (R6-R8).
    pub fn update(&mut self, dt: f32, handlers: &mut dyn Handlers) -> Vec<Out> {
        let mut out = Vec::new();
        loop {
            if self.playing.is_none() {
                match self.queue.pop_front() {
                    Some(group) => self.playing = Some(Running { group, index: 0, started: false }),
                    None => return out,
                }
            }
            let mask = self.skip_mask;
            let Some(run) = self.playing.as_mut() else { return out };
            let mut waiting = false;
            while let Some(event) = run.group.events.get(run.index) {
                if event.mask & mask != 0 {
                    run.index += 1;
                    continue;
                }
                if !run.started {
                    handlers.start(event);
                    run.started = true;
                }
                handlers.update(event, dt);
                if handlers.is_done(event) {
                    run.index += 1;
                    run.started = false;
                } else {
                    waiting = true;
                    break;
                }
            }
            if waiting {
                return out;
            }
            if let Some(done) = self.playing.take() {
                out.push(Out::GroupFinished(done.group.id));
            }
        }
    }

    /// Skip a group (82DBB060, R9): playing, its running event stops and the ones already run are undone in reverse,
    /// then it finishes; queued, it leaves the queue unplayed.
    pub fn skip(&mut self, id: &str, handlers: &mut dyn Handlers) -> Vec<Out> {
        let mut out = Vec::new();
        if self.playing.as_ref().is_some_and(|r| r.group.id == id) {
            if let Some(run) = self.playing.take() {
                if run.started {
                    if let Some(event) = run.group.events.get(run.index) {
                        handlers.stop(event);
                    }
                }
                for event in run.group.events[..run.index.min(run.group.events.len())].iter().rev().filter(|e| e.mask & self.skip_mask == 0) {
                    handlers.undo(event);
                }
                out.push(Out::GroupFinished(run.group.id));
            }
        }
        self.queue.retain(|g| g.id != id);
        out
    }

    /// Remove every group of an owner (82DBA8F8, R10: the running event stops; queued ones go unplayed): a challenge's
    /// cleanup or abandon, a mod being disabled.
    pub fn remove_owner(&mut self, owner: &str, handlers: &mut dyn Handlers) -> Vec<Out> {
        let mut out = Vec::new();
        if self.playing.as_ref().is_some_and(|r| r.group.owner == owner) {
            if let Some(run) = self.playing.take() {
                if run.started {
                    if let Some(event) = run.group.events.get(run.index) {
                        handlers.stop(event);
                    }
                }
                out.push(Out::GroupRemoved(run.group.id));
            }
        }
        let (gone, kept): (Vec<Group>, Vec<Group>) = self.queue.drain(..).partition(|g| g.owner == owner);
        self.queue = kept.into();
        out.extend(gone.into_iter().map(|g| Out::GroupRemoved(g.id)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "wait:N" events take N updates; anything else is instant. Records the calls.
    #[derive(Default)]
    struct Log {
        calls: Vec<String>,
        left: std::collections::HashMap<String, u32>,
    }

    impl Handlers for Log {
        fn start(&mut self, e: &Event) {
            self.calls.push(format!("start {}", e.kind));
            if let Some(n) = e.kind.strip_prefix("wait:") {
                self.left.insert(e.kind.clone(), n.parse().unwrap());
            }
        }
        fn is_done(&mut self, e: &Event) -> bool {
            match self.left.get_mut(&e.kind) {
                Some(0) | None => true,
                Some(n) => {
                    *n -= 1;
                    false
                }
            }
        }
        fn undo(&mut self, e: &Event) {
            self.calls.push(format!("undo {}", e.kind));
        }
        fn stop(&mut self, e: &Event) {
            self.calls.push(format!("stop {}", e.kind));
        }
    }

    fn ev(kind: &str, mask: u32) -> Event {
        Event { kind: kind.into(), args: [None, None], mask }
    }

    fn group(id: &str, owner: &str, kinds: &[&str]) -> Group {
        Group { id: id.into(), owner: owner.into(), events: kinds.iter().map(|k| ev(k, 0)).collect() }
    }

    #[test]
    fn the_queue_skips_repeats_and_drops_when_full() {
        let mut m = Manager { capacity: 2, ..Default::default() };
        assert_eq!(m.start(group("a", "t", &["wait:5"])), Started::Now);
        assert_eq!(m.start(group("a", "t", &[])), Started::Skipped, "equal to the playing group, nothing queued");
        assert_eq!(m.start(group("b", "t", &[])), Started::Queued);
        assert_eq!(m.start(group("b", "t", &[])), Started::Skipped, "equal to the last queued");
        assert_eq!(m.start(group("a", "t", &[])), Started::Queued, "the playing one again, behind another");
        assert_eq!(m.start(group("c", "t", &[])), Started::Dropped);
    }

    #[test]
    fn groups_play_in_order_one_at_a_time_and_finish() {
        let mut m = Manager::default();
        let mut h = Log::default();
        m.start(group("a", "t", &["fade", "wait:2", "callout"]));
        m.start(group("b", "t", &["sfx"]));
        assert!(m.update(0.1, &mut h).is_empty());
        assert_eq!(h.calls, ["start fade", "start wait:2"]);
        assert!(m.update(0.1, &mut h).is_empty());
        assert_eq!(m.update(0.1, &mut h), [Out::GroupFinished("a".into()), Out::GroupFinished("b".into())]);
        assert_eq!(h.calls, ["start fade", "start wait:2", "start callout", "start sfx"]);
        assert_eq!(m.playing(), None);
    }

    #[test]
    fn a_masked_event_is_skipped() {
        let mut m = Manager { skip_mask: 4, ..Default::default() };
        let mut h = Log::default();
        m.start(Group { id: "a".into(), owner: "t".into(), events: vec![ev("one", 0), ev("two", 4), ev("three", 1)] });
        m.update(0.1, &mut h);
        assert_eq!(h.calls, ["start one", "start three"]);
    }

    #[test]
    fn a_skip_stops_the_running_event_and_undoes_the_others_in_reverse() {
        let mut m = Manager::default();
        let mut h = Log::default();
        m.start(group("a", "t", &["one", "two", "wait:9", "four"]));
        m.start(group("b", "t", &["x"]));
        m.update(0.1, &mut h);
        h.calls.clear();
        assert_eq!(m.skip("a", &mut h), [Out::GroupFinished("a".into())]);
        assert_eq!(h.calls, ["stop wait:9", "undo two", "undo one"]);
        assert_eq!(m.update(0.1, &mut h), [Out::GroupFinished("b".into())], "the next group plays");
    }

    #[test]
    fn removing_an_owner_stops_its_group_and_empties_its_queue() {
        let mut m = Manager::default();
        let mut h = Log::default();
        m.start(group("tut/intro", "tut", &["wait:9"]));
        m.start(group("career/s", "career", &["x"]));
        m.start(group("tut/outro", "tut", &["y"]));
        m.update(0.1, &mut h);
        let out = m.remove_owner("tut", &mut h);
        assert_eq!(out, [Out::GroupRemoved("tut/intro".into()), Out::GroupRemoved("tut/outro".into())]);
        assert_eq!(h.calls.last().unwrap(), "stop wait:9");
        assert_eq!(m.update(0.1, &mut h), [Out::GroupFinished("career/s".into())]);
    }
}
