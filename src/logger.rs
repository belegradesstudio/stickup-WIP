//! Simple stdout logger for input events.
//!
//! `Logger` is a convenience listener that prints every incoming
//! [`InputEvent`](crate::event::InputEvent) to `stdout`.
//!
//! # Examples
//! ```ignore
//! use stickup::devices::logger::Logger;
//! use stickup::eventbus::{InputEventBus, EventFilter};
//!
//! let mut bus = InputEventBus::new();
//! let _id = bus.add_listener(Box::new(Logger::new()), EventFilter::All);
//! // Now every emitted event will be printed to stdout.
//! ```

use crate::event::InputEvent;
use crate::eventbus::InputListener;
use std::ops::ControlFlow;

/// A simple listener that logs all input events to stdout.
///
/// Useful for debugging, demos, or quick inspections during development.
/// For structured logging, consider integrating with your logging framework
/// (e.g., `tracing`) in a custom listener.
pub struct Logger;

impl Logger {
    /// Construct a new [`Logger`].
    #[inline]
    pub fn new() -> Self {
        Logger
    }
}

impl InputListener for Logger {
    /// Prints the event as `"[Input] {:?}"` and continues.
    #[inline]
    fn on_input(&mut self, e: &InputEvent) -> ControlFlow<()> {
        println!("[Input] {:?}", e);
        ControlFlow::Continue(())
    }
}
