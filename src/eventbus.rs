//! Event bus and filters for input events.
//!
//! This module provides a lightweight, single-threaded event bus for dispatching
//! [`InputEvent`](crate::event::InputEvent)s to registered listeners, with a small
//! library of composable filters.
//!
//! - **Listeners** implement [`InputListener`] or can be registered from closures.
//! - **Filters** ([`EventFilter`]) allow easy matching by kind, device, hat index, or
//!   arbitrary predicates (via [`Predicate`] and [`EventFilter::custom`]).
//! - **One-shot helpers** simplify common “learn/bind” interactions.
//!
//! # Threading
//! The bus is intended for **single-threaded** use. All traits require `Send` so you
//! can move listeners across threads, but `InputEventBus` itself is not synchronized.
//! If you need multi-threaded dispatch, run the bus on one thread and send events to
//! it via channels.
//!
//! # Examples
//! Register a closure that prints all button events from devices starting with `"046d:"`:
//! ```ignore
//! use stickup::event::{InputEvent, InputKind};
//! use stickup::eventbus::{InputEventBus, EventFilter};
//! use std::ops::ControlFlow;
//!
//! let mut bus = InputEventBus::new();
//! bus.add_fn_listener(
//!     |e: &InputEvent| {
//!         if matches!(e.kind, InputKind::ButtonPressed { .. } | InputKind::ButtonReleased { .. }) {
//!             println!("button on {}: {:?}", e.device_id, e.kind);
//!         }
//!         ControlFlow::Continue(())
//!     },
//!     EventFilter::And(vec![
//!         EventFilter::ButtonsOnly,
//!         EventFilter::DevicePrefix("046d:".into()),
//!     ]),
//! );
//! ```
//!
//! One-shot: capture two **distinct axes** from the **same device** (for a dual-axis bind):
//! ```ignore
//! use stickup::eventbus::{InputEventBus, EventFilter};
//!
//! let mut bus = InputEventBus::new();
//! bus.add_oneshot_axes2(
//!     |dev, a0, a1| println!("Got axes from {}: {:?} and {:?}", dev, a0, a1),
//!     EventFilter::AxisOnly,
//! );
//! ```

use std::collections::HashMap;
use std::fmt;
use std::ops::ControlFlow;
use std::sync::Arc;

use crate::event::{InputEvent, InputKind};

/// Listener for input events.
///
/// Return [`ControlFlow::Break(())`] to **auto-unregister** after this call.
/// Returning [`ControlFlow::Continue(())`] keeps the listener registered.
pub trait InputListener: Send {
    /// Handle one input event.
    fn on_input(&mut self, event: &InputEvent) -> ControlFlow<()>;
}

/// Convenience wrapper that turns a `FnMut(&InputEvent) -> ControlFlow<()>` into an [`InputListener`].
pub struct FnListener<F>(F);

impl<F> InputListener for FnListener<F>
where
    F: FnMut(&InputEvent) -> ControlFlow<()> + Send + 'static,
{
    fn on_input(&mut self, e: &InputEvent) -> ControlFlow<()> {
        (self.0)(e)
    }
}

/// Predicate used by [`EventFilter::Custom`] and composition helpers.
///
/// Implement this trait (or use a closure) to build custom matchers.
pub trait Predicate: Send + Sync {
    /// Returns `true` if the event matches the predicate.
    fn test(&self, e: &InputEvent) -> bool;
}

impl<F> Predicate for F
where
    F: Fn(&InputEvent) -> bool + Send + Sync + 'static,
{
    fn test(&self, e: &InputEvent) -> bool {
        self(e)
    }
}

/// Flexible, composable event filter.
///
/// Use predefined variants or construct custom logic with [`EventFilter::custom`],
/// [`EventFilter::And`], [`EventFilter::Or`], and [`EventFilter::Not`].
///
/// `Debug` is manual so we can hold closures.
#[derive(Clone)]
pub enum EventFilter {
    /// Allow everything.
    All,
    /// Only axis events.
    AxisOnly,
    /// Only button press/release events.
    ButtonsOnly,
    /// Only hat (POV) events.
    HatsOnly,
    /// Buttons OR hats (treat hats as button-ish).
    ButtonsOrHats,
    /// Match device IDs starting with this prefix (e.g., `"046d:"` or full `"046d:c21d:SER"`).
    DevicePrefix(String),
    /// Only pass events for a specific hat index (`0,1,…`).
    HatIndex(u16),
    /// Only pass hat events when moved off center (non-neutral).
    HatNonNeutral,
    /// Arbitrary predicate (closure/struct) over the whole event.
    Custom(Arc<dyn Predicate>),
    /// Logical composition: **all** subfilters must match.
    And(Vec<EventFilter>),
    /// Logical composition: **any** subfilter matches.
    Or(Vec<EventFilter>),
    /// Logical negation of a subfilter.
    Not(Box<EventFilter>),
}

impl EventFilter {
    /// Wrap a closure as a custom predicate filter.
    ///
    /// # Example
    /// ```ignore
    /// use stickup::eventbus::EventFilter;
    /// let f = EventFilter::custom(|e| e.device_id == "virtual:0");
    /// ```
    #[inline]
    pub fn custom<F>(f: F) -> Self
    where
        F: Fn(&InputEvent) -> bool + Send + Sync + 'static,
    {
        Self::Custom(Arc::new(f))
    }

    /// Returns `true` if this filter matches the given event.
    #[inline]
    pub fn matches(&self, e: &InputEvent) -> bool {
        match self {
            EventFilter::All => true,

            EventFilter::AxisOnly => matches!(e.kind, InputKind::AxisMoved { .. }),

            EventFilter::ButtonsOnly => matches!(
                e.kind,
                InputKind::ButtonPressed { .. } | InputKind::ButtonReleased { .. }
            ),

            EventFilter::HatsOnly => matches!(e.kind, InputKind::HatChanged { .. }),

            EventFilter::ButtonsOrHats => matches!(
                e.kind,
                InputKind::ButtonPressed { .. }
                    | InputKind::ButtonReleased { .. }
                    | InputKind::HatChanged { .. }
            ),

            EventFilter::DevicePrefix(p) => e.device_id.starts_with(p),

            EventFilter::HatIndex(idx) => {
                matches!(e.kind, InputKind::HatChanged { hat, .. } if hat == *idx)
            }

            EventFilter::HatNonNeutral => matches!(e.kind, InputKind::HatChanged { value, .. }
                if !is_hat_neutral(value)
            ),

            EventFilter::Custom(p) => p.test(e),

            EventFilter::And(list) => list.iter().all(|f| f.matches(e)),
            EventFilter::Or(list) => list.iter().any(|f| f.matches(e)),
            EventFilter::Not(inner) => !inner.matches(e),
        }
    }
}

/// Returns `true` if a hat value is commonly used to represent *neutral*.
///
/// Recognized neutral encodings across stacks:
/// - `-1` (SDL style),
/// - `8` (some HID usages),
/// - `15` or `255` (other stacks).
#[inline]
fn is_hat_neutral(value: i16) -> bool {
    matches!(value, -1 | 8 | 15 | 255)
}

impl fmt::Debug for EventFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventFilter::All => f.write_str("All"),
            EventFilter::AxisOnly => f.write_str("AxisOnly"),
            EventFilter::ButtonsOnly => f.write_str("ButtonsOnly"),
            EventFilter::HatsOnly => f.write_str("HatsOnly"),
            EventFilter::ButtonsOrHats => f.write_str("ButtonsOrHats"),
            EventFilter::DevicePrefix(p) => f.debug_tuple("DevicePrefix").field(p).finish(),
            EventFilter::HatIndex(i) => f.debug_tuple("HatIndex").field(i).finish(),
            EventFilter::HatNonNeutral => f.write_str("HatNonNeutral"),
            EventFilter::Custom(_) => f.write_str("Custom(..)"),
            EventFilter::And(_) => f.write_str("And(..)"),
            EventFilter::Or(_) => f.write_str("Or(..)"),
            EventFilter::Not(_) => f.write_str("Not(..)"),
        }
    }
}

/// Registered listener entry (internal).
struct ListenerEntry {
    listener: Box<dyn InputListener>,
    enabled: bool,
    filter: EventFilter,
    id: u64,
}

/// Single-threaded event bus for input events.
///
/// Listeners are invoked in insertion order. Those that return [`ControlFlow::Break`]
/// are removed **after** dispatch completes to avoid invalidating iteration.
pub struct InputEventBus {
    next_id: u64,
    listeners: HashMap<u64, ListenerEntry>,
}

impl InputEventBus {
    /// Create an empty event bus.
    pub fn new() -> Self {
        Self {
            next_id: 0,
            listeners: HashMap::new(),
        }
    }

    /// Register a trait-object listener with a filter; returns a handle `id`.
    ///
    /// Use [`remove_listener`] to unregister, or return `Break(())` from the listener
    /// to auto-unregister after the next matching event.
    pub fn add_listener(&mut self, listener: Box<dyn InputListener>, filter: EventFilter) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.listeners.insert(
            id,
            ListenerEntry {
                listener,
                enabled: true,
                filter,
                id,
            },
        );
        id
    }

    /// Register a closure as a listener (wrapped in [`FnListener`]).
    ///
    /// # Example
    /// ```ignore
    /// use stickup::eventbus::{InputEventBus, EventFilter};
    /// use std::ops::ControlFlow;
    ///
    /// let mut bus = InputEventBus::new();
    /// let id = bus.add_fn_listener(
    ///     |_| ControlFlow::Continue(()),
    ///     EventFilter::All,
    /// );
    /// ```
    pub fn add_fn_listener<F>(&mut self, f: F, filter: EventFilter) -> u64
    where
        F: FnMut(&InputEvent) -> ControlFlow<()> + Send + 'static,
    {
        self.add_listener(Box::new(FnListener(f)), filter)
    }

    /// One-shot convenience: auto-unregister after the **first** matching event.
    ///
    /// The provided callback runs once and does **not** receive a `ControlFlow` to continue.
    pub fn add_oneshot_fn<F>(&mut self, mut f: F, filter: EventFilter) -> u64
    where
        F: FnMut(&InputEvent) + Send + 'static,
    {
        self.add_fn_listener(
            move |e| {
                f(e);
                ControlFlow::Break(())
            },
            filter,
        )
    }

    /// One-shot with **internal state/progress**.
    ///
    /// Calls `step(state, event)` for each matching event; when it returns `true`,
    /// the listener auto-unregisters.
    pub fn add_oneshot_state<S, Step>(
        &mut self,
        mut state: S,
        mut step: Step,
        filter: EventFilter,
    ) -> u64
    where
        S: Send + 'static,
        Step: FnMut(&mut S, &InputEvent) -> bool + Send + 'static,
    {
        self.add_fn_listener(
            move |e| {
                if step(&mut state, e) {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
            filter,
        )
    }

    /// One-shot that fires after **N matching events**, then auto-unregisters.
    pub fn add_oneshot_n<F>(&mut self, n: usize, mut f: F, filter: EventFilter) -> u64
    where
        F: FnMut(&InputEvent) + Send + 'static,
    {
        struct Cnt {
            seen: usize,
            need: usize,
        }
        self.add_oneshot_state(
            Cnt {
                seen: 0,
                need: n.max(1),
            },
            move |s, e| {
                f(e);
                s.seen += 1;
                s.seen >= s.need
            },
            filter,
        )
    }

    /// Purpose-built one-shot for learning a **dual axis**:
    /// captures two **distinct** axes from the **same device**.
    ///
    /// Invokes `cb(device_id, (axis0, val0), (axis1, val1))` when both are seen.
    pub fn add_oneshot_axes2<CB>(&mut self, mut cb: CB, filter: EventFilter) -> u64
    where
        CB: FnMut(&str, (u16, f32), (u16, f32)) + Send + 'static,
    {
        #[derive(Clone)]
        struct Ax2State {
            device: Option<String>,
            first: Option<(u16, f32)>,
        }

        self.add_oneshot_state(
            Ax2State {
                device: None,
                first: None,
            },
            move |s, e| {
                let (axis, value) = match e.kind {
                    InputKind::AxisMoved { axis, value } => (axis, value),
                    _ => return false,
                };

                match &mut s.device {
                    None => s.device = Some(e.device_id.clone()),
                    Some(d) if *d != e.device_id => return false,
                    _ => {}
                }

                match s.first {
                    None => {
                        s.first = Some((axis, value));
                        false
                    }
                    Some((a0, v0)) => {
                        if axis == a0 {
                            return false;
                        }
                        cb(s.device.as_ref().unwrap(), (a0, v0), (axis, value));
                        true
                    }
                }
            },
            filter,
        )
    }

    /// One-shot for hats: fires on the first **non-neutral** [`HatChanged`](InputKind::HatChanged)
    /// event that passes `filter` (e.g., `And([HatsOnly, HatIndex(0), HatNonNeutral])`).
    pub fn add_oneshot_hat_nonneutral<CB>(&mut self, mut cb: CB, filter: EventFilter) -> u64
    where
        CB: FnMut(&str, u16, i16) + Send + 'static,
    {
        self.add_fn_listener(
            move |e| {
                if let InputKind::HatChanged { hat, value } = e.kind {
                    if !is_hat_neutral(value) {
                        cb(&e.device_id, hat, value);
                        return ControlFlow::Break(());
                    }
                }
                ControlFlow::Continue(())
            },
            filter, // filtering happens in the bus before invoking this closure
        )
    }

    // -------- Basic helpers --------

    /// Enable a registered listener by `id`.
    pub fn enable(&mut self, id: u64) {
        if let Some(e) = self.listeners.get_mut(&id) {
            e.enabled = true;
        }
    }

    /// Disable a registered listener by `id` (remains registered but not invoked).
    pub fn disable(&mut self, id: u64) {
        if let Some(e) = self.listeners.get_mut(&id) {
            e.enabled = false;
        }
    }

    /// Unregister and remove a listener by `id`.
    pub fn remove_listener(&mut self, id: u64) {
        self.listeners.remove(&id);
    }

    /// Emit **one event** to all enabled, matching listeners.
    ///
    /// Listeners that return `Break(())` are removed **after** dispatch completes.
    pub fn emit(&mut self, event: &InputEvent) {
        // Collect removals to avoid mutating the map during iteration.
        let mut to_remove: Vec<u64> = Vec::new();

        for entry in self.listeners.values_mut() {
            if !entry.enabled {
                continue;
            }
            if !entry.filter.matches(event) {
                continue;
            }

            if let ControlFlow::Break(()) = entry.listener.on_input(event) {
                to_remove.push(entry.id);
            }
        }

        for id in to_remove {
            self.listeners.remove(&id);
        }
    }

    /// Emit a **batch** of events (more cache-friendly than calling [`emit`] in a loop).
    pub fn emit_all(&mut self, events: &[InputEvent]) {
        for e in events {
            self.emit(e);
        }
    }
}
