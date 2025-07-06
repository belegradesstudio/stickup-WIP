//! src/devices/logger.rs
use crate::devices::event::InputEvent;
use crate::devices::eventbus::InputListener;

/// A simple listener that logs all input events to stdout.
pub struct Logger;

impl Logger {
    pub fn new() -> Self {
        Logger
    }
}

impl InputListener for Logger {
    fn on_input(&mut self, event: &InputEvent) {
        println!("[Input] {:?}", event);
    }
}
