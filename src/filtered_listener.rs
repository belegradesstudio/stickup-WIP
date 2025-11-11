//! Predicate-gated listener adapter.
//!
//! `FilteredListener` wraps any [`InputListener`](crate::eventbus::InputListener) and
//! only forwards events that satisfy a user-supplied predicate.
//!
//! This is handy when you want *per-listener* filtering logic without wiring up
//! a shared [`EventFilter`](crate::eventbus::EventFilter) on the bus itself.
//!
//! # Examples
//! Forward only axis events to an inner listener:
//! ```ignore
//! use stickup::event::{InputEvent, InputKind};
//! use stickup::eventbus::{InputListener};
//! use stickup::filtered_listener::FilteredListener;
//! use std::ops::ControlFlow;
//!
//! struct PrintAxis;
//! impl InputListener for PrintAxis {
//!     fn on_input(&mut self, e: &InputEvent) -> ControlFlow<()> {
//!         println!("axis from {}: {:?}", e.device_id, e.kind);
//!         ControlFlow::Continue(())
//!     }
//! }
//!
//! let inner = PrintAxis;
//! let mut l = FilteredListener::new(
//!     |e: &InputEvent| matches!(e.kind, InputKind::AxisMoved { .. }),
//!     inner,
//! );
//! // Now `l.on_input(e)` only forwards axis events to `PrintAxis`.
//! ```
//!
//! ## API Notes
//! - Useful when different listeners need different predicates without expanding the
//!   bus-wide filter set.
//! - Predicate is `Fn(&InputEvent) -> bool + Send + Sync + 'static`.
//! - Returning [`ControlFlow::Break(())`] from the **inner** listener will still cause
//!   bus-side auto-unregister (when using the event bus helpers).

use crate::event::InputEvent;
use crate::eventbus::InputListener;
use std::ops::ControlFlow;

/// A listener that forwards events to `inner` **only** when `predicate(event)` is `true`.
pub struct FilteredListener<L, P>
where
    L: InputListener,
    P: Fn(&InputEvent) -> bool + Send + Sync + 'static,
{
    predicate: P,
    inner: L,
}

impl<L, P> FilteredListener<L, P>
where
    L: InputListener,
    P: Fn(&InputEvent) -> bool + Send + Sync + 'static,
{
    /// Create a new filtered listener.
    ///
    /// - `predicate`: called for each event; if it returns `true`, the event is forwarded.
    /// - `inner`: the wrapped listener to receive matching events.
    pub fn new(predicate: P, inner: L) -> Self {
        Self { predicate, inner }
    }
}

impl<L, P> InputListener for FilteredListener<L, P>
where
    L: InputListener,
    P: Fn(&InputEvent) -> bool + Send + Sync + 'static,
{
    /// If `predicate(event)` is `true`, forwards to `inner`; otherwise continues.
    fn on_input(&mut self, e: &InputEvent) -> ControlFlow<()> {
        if (self.predicate)(e) {
            self.inner.on_input(e)
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// Fires once when a specific axis moves by at least `threshold` (Δ) from its baseline.
///
/// Baseline is set on the **first** observation of that axis; subsequent samples are
/// compared against that baseline. On trigger, calls the user callback and (by default)
/// stops further propagation by returning `Break(())`. If you want it to continue
/// propagating, construct with `propagate = true`.
pub struct AxisMotionThreshold<F>
where
    F: Fn(u16, f32) + Send + Sync + 'static,
{
    axis: u16,
    threshold: f32,
    baseline: Option<f32>,
    callback: F,
    propagate: bool,
}

impl<F> AxisMotionThreshold<F>
where
    F: Fn(u16, f32) + Send + Sync + 'static,
{
    pub fn new(axis: u16, threshold: f32, callback: F) -> Self {
        Self {
            axis,
            threshold: threshold.abs(),
            baseline: None,
            callback,
            propagate: false,
        }
    }
    /// Continue propagating after firing instead of breaking the dispatch loop.
    pub fn with_propagation(mut self, propagate: bool) -> Self {
        self.propagate = propagate;
        self
    }
}

impl<F> InputListener for AxisMotionThreshold<F>
where
    F: Fn(u16, f32) + Send + Sync + 'static,
{
    fn on_input(&mut self, e: &InputEvent) -> ControlFlow<()> {
        use crate::event::InputKind;
        if let InputKind::AxisMoved { axis, value } = e.kind {
            if axis == self.axis {
                match self.baseline {
                    None => self.baseline = Some(value),
                    Some(b) => {
                        if (value - b).abs() >= self.threshold {
                            (self.callback)(axis, value);
                            return if self.propagate {
                                ControlFlow::Continue(())
                            } else {
                                ControlFlow::Break(())
                            };
                        }
                    }
                }
            }
        }
        ControlFlow::Continue(())
    }
}
