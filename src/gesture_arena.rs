//! Gesture arena primitives for resolving competing gesture recognizers.
//!
//! This module is a Rust port of Flutter's gesture arena semantics:
//! <https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/gestures/arena.dart>
//!
//! Portions derived from Flutter are covered by Flutter's BSD-style license.
//! Copyright 2014 The Flutter Authors. All rights reserved.

use crate::PointerId;
use collections::FxHashMap;
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::{Rc, Weak},
};

/// The resolution requested by a gesture recognizer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GestureDisposition {
    /// The recognizer is claiming the pointer sequence.
    Accepted,
    /// The recognizer is giving up on the pointer sequence.
    Rejected,
}

/// A participant in a gesture arena.
pub trait GestureArenaMember: 'static {
    /// Notify the member that it won the arena for the pointer.
    fn accept_gesture(&mut self, pointer: PointerId);

    /// Notify the member that it lost the arena for the pointer.
    fn reject_gesture(&mut self, pointer: PointerId);
}

/// Stable identifier for a member registered in a gesture arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GestureArenaMemberId(u64);

/// A reusable handle for a gesture arena member.
#[derive(Clone)]
pub struct GestureArenaMemberHandle {
    id: GestureArenaMemberId,
    member: Rc<RefCell<dyn GestureArenaMember>>,
}

impl GestureArenaMemberHandle {
    /// Return this member's stable id.
    pub fn id(&self) -> GestureArenaMemberId {
        self.id
    }
}

/// An entry returned when a recognizer joins an arena.
///
/// Dropping the entry does not resolve the arena. Call [`Self::resolve`] when the recognizer has
/// accepted or rejected the pointer sequence.
#[derive(Clone)]
pub struct GestureArenaEntry {
    pointer: PointerId,
    member: GestureArenaMemberId,
    manager: Weak<RefCell<GestureArenaManagerInner>>,
}

impl GestureArenaEntry {
    /// Resolve this entry in its arena.
    pub fn resolve(&self, disposition: GestureDisposition) {
        let Some(manager) = self.manager.upgrade() else {
            return;
        };
        let decisions = manager
            .borrow_mut()
            .resolve(self.pointer, self.member, disposition);
        dispatch_decisions(decisions);
    }
}

#[derive(Clone)]
struct ArenaMember {
    id: GestureArenaMemberId,
    handle: GestureArenaMemberHandle,
}

#[derive(Default)]
struct GestureArena {
    members: Vec<ArenaMember>,
    is_open: bool,
    is_held: bool,
    has_pending_sweep: bool,
    eager_winner: Option<GestureArenaMemberId>,
}

struct GestureArenaDecision {
    pointer: PointerId,
    member: GestureArenaMemberHandle,
    disposition: GestureDisposition,
}

#[derive(Default)]
struct GestureArenaManagerInner {
    arenas: FxHashMap<PointerId, GestureArena>,
    pending_default_resolutions: VecDeque<PointerId>,
    next_member_id: u64,
}

/// Coordinates recognizers competing for pointer sequences.
///
/// An arena is opened lazily when the first member joins a pointer. Closing an arena stops further
/// normal participation. If a member accepts while the arena is still open, it becomes the eager
/// winner and wins once the arena closes unless the arena is held. Sweeping chooses the first
/// remaining member, matching the usual passive-recognizer fallback behavior.
#[derive(Clone, Default)]
pub struct GestureArenaManager {
    inner: Rc<RefCell<GestureArenaManagerInner>>,
}

impl GestureArenaManager {
    /// Construct an empty gesture arena manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a reusable member handle.
    pub fn member(&self, member: impl GestureArenaMember) -> GestureArenaMemberHandle {
        let mut inner = self.inner.borrow_mut();
        let id = GestureArenaMemberId(inner.next_member_id);
        inner.next_member_id = inner.next_member_id.wrapping_add(1);
        GestureArenaMemberHandle {
            id,
            member: Rc::new(RefCell::new(member)),
        }
    }

    /// Add a member to the arena for the pointer.
    pub fn add(&self, pointer: PointerId, member: GestureArenaMemberHandle) -> GestureArenaEntry {
        self.inner.borrow_mut().add(pointer, member.clone());
        GestureArenaEntry {
            pointer,
            member: member.id,
            manager: Rc::downgrade(&self.inner),
        }
    }

    /// Prevent a sweep from resolving the pointer's arena.
    pub fn hold(&self, pointer: PointerId) {
        self.inner.borrow_mut().hold(pointer);
    }

    /// Release a hold on the pointer's arena.
    pub fn release(&self, pointer: PointerId) {
        let decisions = self.inner.borrow_mut().release(pointer);
        dispatch_decisions(decisions);
    }

    /// Close the pointer's arena to new normal participation.
    pub fn close(&self, pointer: PointerId) {
        let decisions = self.inner.borrow_mut().close(pointer);
        dispatch_decisions(decisions);
    }

    /// Force the arena to resolve in favor of the first remaining member.
    pub fn sweep(&self, pointer: PointerId) {
        let decisions = self.inner.borrow_mut().sweep(pointer);
        dispatch_decisions(decisions);
    }

    /// Deliver delayed default resolutions.
    pub fn flush_pending_resolutions(&self) {
        loop {
            let decisions = self.inner.borrow_mut().flush_one_pending_resolution();
            if decisions.is_empty() {
                break;
            }
            dispatch_decisions(decisions);
        }
    }

    /// Return the number of members currently participating for a pointer.
    pub fn member_count(&self, pointer: PointerId) -> usize {
        self.inner
            .borrow()
            .arenas
            .get(&pointer)
            .map_or(0, |arena| arena.members.len())
    }
}

impl GestureArenaManagerInner {
    fn add(&mut self, pointer: PointerId, member: GestureArenaMemberHandle) {
        let arena = self.arenas.entry(pointer).or_insert_with(|| GestureArena {
            is_open: true,
            ..GestureArena::default()
        });
        debug_assert!(arena.is_open, "cannot add to a closed gesture arena");
        if arena.is_open {
            arena.members.push(ArenaMember {
                id: member.id,
                handle: member,
            });
        }
    }

    fn hold(&mut self, pointer: PointerId) {
        if let Some(arena) = self.arenas.get_mut(&pointer) {
            arena.is_held = true;
        }
    }

    fn release(&mut self, pointer: PointerId) -> Vec<GestureArenaDecision> {
        let Some(arena) = self.arenas.get_mut(&pointer) else {
            return Vec::new();
        };
        arena.is_held = false;
        if arena.has_pending_sweep {
            return self.sweep(pointer);
        }
        self.try_to_resolve_arena(pointer)
    }

    fn close(&mut self, pointer: PointerId) -> Vec<GestureArenaDecision> {
        let Some(arena) = self.arenas.get_mut(&pointer) else {
            return Vec::new();
        };
        arena.is_open = false;
        self.try_to_resolve_arena(pointer)
    }

    fn sweep(&mut self, pointer: PointerId) -> Vec<GestureArenaDecision> {
        let Some(mut arena) = self.arenas.remove(&pointer) else {
            return Vec::new();
        };
        if arena.is_held {
            arena.has_pending_sweep = true;
            self.arenas.insert(pointer, arena);
            return Vec::new();
        }

        let mut members = arena.members.drain(..);
        let Some(winner) = members.next() else {
            return Vec::new();
        };

        let mut decisions = vec![GestureArenaDecision {
            pointer,
            member: winner.handle,
            disposition: GestureDisposition::Accepted,
        }];
        decisions.extend(members.map(|member| GestureArenaDecision {
            pointer,
            member: member.handle,
            disposition: GestureDisposition::Rejected,
        }));
        decisions
    }

    fn resolve(
        &mut self,
        pointer: PointerId,
        member: GestureArenaMemberId,
        disposition: GestureDisposition,
    ) -> Vec<GestureArenaDecision> {
        let Some(arena) = self.arenas.get_mut(&pointer) else {
            return Vec::new();
        };

        match disposition {
            GestureDisposition::Accepted => {
                if arena.is_open {
                    arena.eager_winner = Some(member);
                    Vec::new()
                } else {
                    self.resolve_in_favor_of(pointer, member)
                }
            }
            GestureDisposition::Rejected => {
                let Some(member_index) = arena.members.iter().position(|entry| entry.id == member)
                else {
                    return Vec::new();
                };
                let rejected = arena.members.remove(member_index);
                let mut decisions = vec![GestureArenaDecision {
                    pointer,
                    member: rejected.handle,
                    disposition: GestureDisposition::Rejected,
                }];
                decisions.extend(self.try_to_resolve_arena(pointer));
                decisions
            }
        }
    }

    fn try_to_resolve_arena(&mut self, pointer: PointerId) -> Vec<GestureArenaDecision> {
        let Some(arena) = self.arenas.get(&pointer) else {
            return Vec::new();
        };
        if arena.is_open || arena.is_held {
            return Vec::new();
        }
        if arena.members.is_empty() {
            self.arenas.remove(&pointer);
            return Vec::new();
        }
        if arena.members.len() == 1 {
            if !self.pending_default_resolutions.contains(&pointer) {
                self.pending_default_resolutions.push_back(pointer);
            }
            return Vec::new();
        }
        if let Some(eager_winner) = arena.eager_winner {
            return self.resolve_in_favor_of(pointer, eager_winner);
        }
        Vec::new()
    }

    fn resolve_in_favor_of(
        &mut self,
        pointer: PointerId,
        member: GestureArenaMemberId,
    ) -> Vec<GestureArenaDecision> {
        let Some(arena) = self.arenas.remove(&pointer) else {
            return Vec::new();
        };
        arena
            .members
            .into_iter()
            .map(|entry| GestureArenaDecision {
                pointer,
                member: entry.handle,
                disposition: if entry.id == member {
                    GestureDisposition::Accepted
                } else {
                    GestureDisposition::Rejected
                },
            })
            .collect()
    }

    fn flush_one_pending_resolution(&mut self) -> Vec<GestureArenaDecision> {
        let Some(pointer) = self.pending_default_resolutions.pop_front() else {
            return Vec::new();
        };
        self.resolve_by_default(pointer)
    }

    fn resolve_by_default(&mut self, pointer: PointerId) -> Vec<GestureArenaDecision> {
        let Some(arena) = self.arenas.get(&pointer) else {
            return Vec::new();
        };
        if arena.is_open || arena.is_held || arena.members.len() != 1 {
            return Vec::new();
        }

        let Some(arena) = self.arenas.remove(&pointer) else {
            return Vec::new();
        };
        let winner = arena.members.into_iter().next().expect("checked len above");
        vec![GestureArenaDecision {
            pointer,
            member: winner.handle,
            disposition: GestureDisposition::Accepted,
        }]
    }
}

fn dispatch_decisions(decisions: Vec<GestureArenaDecision>) {
    for decision in decisions {
        match decision.disposition {
            GestureDisposition::Accepted => decision
                .member
                .member
                .borrow_mut()
                .accept_gesture(decision.pointer),
            GestureDisposition::Rejected => decision
                .member
                .member
                .borrow_mut()
                .reject_gesture(decision.pointer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Clone)]
    struct RecordingMember {
        name: &'static str,
        events: Rc<RefCell<Vec<String>>>,
    }

    impl GestureArenaMember for RecordingMember {
        fn accept_gesture(&mut self, pointer: PointerId) {
            self.events
                .borrow_mut()
                .push(format!("{} accept {}", self.name, pointer.as_u64()));
        }

        fn reject_gesture(&mut self, pointer: PointerId) {
            self.events
                .borrow_mut()
                .push(format!("{} reject {}", self.name, pointer.as_u64()));
        }
    }

    #[test]
    fn eager_winner_wins_after_close() {
        let manager = GestureArenaManager::new();
        let events = Rc::new(RefCell::new(Vec::new()));
        let pointer = PointerId::new(1);
        let first = manager.member(RecordingMember {
            name: "first",
            events: Rc::clone(&events),
        });
        let second = manager.member(RecordingMember {
            name: "second",
            events: Rc::clone(&events),
        });

        manager.add(pointer, first);
        let second_entry = manager.add(pointer, second);
        second_entry.resolve(GestureDisposition::Accepted);
        manager.close(pointer);

        assert_eq!(
            events.borrow().as_slice(),
            ["first reject 1", "second accept 1"]
        );
    }

    #[test]
    fn sweep_accepts_first_member() {
        let manager = GestureArenaManager::new();
        let events = Rc::new(RefCell::new(Vec::new()));
        let pointer = PointerId::new(2);
        manager.add(
            pointer,
            manager.member(RecordingMember {
                name: "first",
                events: Rc::clone(&events),
            }),
        );
        manager.add(
            pointer,
            manager.member(RecordingMember {
                name: "second",
                events: Rc::clone(&events),
            }),
        );

        manager.sweep(pointer);

        assert_eq!(
            events.borrow().as_slice(),
            ["first accept 2", "second reject 2"]
        );
    }
}
