//! Pointer event routing primitives.
//!
//! This module is a Rust port of Flutter's pointer router semantics:
//! <https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/gestures/pointer_router.dart>
//!
//! Portions derived from Flutter are covered by Flutter's BSD-style license.
//! Copyright 2014 The Flutter Authors. All rights reserved.

use crate::{PointerEvent, PointerId};
use collections::FxHashMap;
use std::{cell::RefCell, rc::Rc};

/// Identifier for a pointer route registered with [`PointerRouter`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PointerRouteId(u64);

/// A callback that receives routed pointer events.
pub type PointerRoute = Box<dyn FnMut(&PointerEvent) + 'static>;

struct RouteEntry {
    id: PointerRouteId,
    route: Rc<RefCell<PointerRoute>>,
}

impl RouteEntry {
    fn new(id: PointerRouteId, route: PointerRoute) -> Self {
        Self {
            id,
            route: Rc::new(RefCell::new(route)),
        }
    }
}

/// Routes pointer events to per-pointer and global listeners.
///
/// Routing snapshots the current listener set before dispatch. This lets listeners add or remove
/// routes while an event is being delivered without changing who receives that in-flight event.
#[derive(Default)]
pub struct PointerRouter {
    routes: FxHashMap<PointerId, Vec<RouteEntry>>,
    global_routes: Vec<RouteEntry>,
    next_route_id: u64,
}

impl PointerRouter {
    /// Register a route for a specific pointer.
    pub fn add_route(&mut self, pointer: PointerId, route: PointerRoute) -> PointerRouteId {
        let id = self.allocate_route_id();
        self.routes
            .entry(pointer)
            .or_default()
            .push(RouteEntry::new(id, route));
        id
    }

    /// Remove a route for a specific pointer.
    pub fn remove_route(&mut self, pointer: PointerId, route_id: PointerRouteId) -> bool {
        let Some(routes) = self.routes.get_mut(&pointer) else {
            return false;
        };

        let old_len = routes.len();
        routes.retain(|route| route.id != route_id);
        let removed = routes.len() != old_len;
        if routes.is_empty() {
            self.routes.remove(&pointer);
        }
        removed
    }

    /// Register a route that receives all pointer events.
    pub fn add_global_route(&mut self, route: PointerRoute) -> PointerRouteId {
        let id = self.allocate_route_id();
        self.global_routes.push(RouteEntry::new(id, route));
        id
    }

    /// Remove a global pointer route.
    pub fn remove_global_route(&mut self, route_id: PointerRouteId) -> bool {
        let old_len = self.global_routes.len();
        self.global_routes.retain(|route| route.id != route_id);
        self.global_routes.len() != old_len
    }

    /// Dispatch a pointer event to the current global routes and routes for that pointer.
    pub fn route(&mut self, event: &PointerEvent) {
        let mut routes = Vec::new();
        routes.extend(
            self.global_routes
                .iter()
                .map(|entry| Rc::clone(&entry.route)),
        );
        if let Some(pointer_routes) = self.routes.get(&event.pointer) {
            routes.extend(pointer_routes.iter().map(|entry| Rc::clone(&entry.route)));
        }

        for route in routes {
            (route.borrow_mut())(event);
        }
    }

    /// Return the number of routes registered for a pointer.
    pub fn route_count(&self, pointer: PointerId) -> usize {
        self.routes.get(&pointer).map_or(0, |routes| routes.len())
    }

    /// Return the number of global routes.
    pub fn global_route_count(&self) -> usize {
        self.global_routes.len()
    }

    fn allocate_route_id(&mut self) -> PointerRouteId {
        let id = PointerRouteId(self.next_route_id);
        self.next_route_id = self.next_route_id.wrapping_add(1);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Modifiers, PointerDeviceKind, PointerPhase, point, px};
    use std::cell::Cell;

    fn event(pointer: PointerId) -> PointerEvent {
        PointerEvent::new(
            pointer,
            PointerDeviceKind::Touch,
            PointerPhase::Down,
            point(px(0.), px(0.)),
            Modifiers::default(),
        )
    }

    #[test]
    fn routes_to_global_and_pointer_routes() {
        let mut router = PointerRouter::default();
        let pointer = PointerId::new(7);
        let global_count = Rc::new(Cell::new(0));
        let pointer_count = Rc::new(Cell::new(0));

        router.add_global_route(Box::new({
            let global_count = Rc::clone(&global_count);
            move |_| global_count.set(global_count.get() + 1)
        }));
        router.add_route(
            pointer,
            Box::new({
                let pointer_count = Rc::clone(&pointer_count);
                move |_| pointer_count.set(pointer_count.get() + 1)
            }),
        );

        router.route(&event(pointer));
        router.route(&event(PointerId::new(8)));

        assert_eq!(global_count.get(), 2);
        assert_eq!(pointer_count.get(), 1);
    }
}
