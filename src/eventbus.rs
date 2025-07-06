use crate::devices::InputKind;
use crate::devices::event::InputEvent;
use std::collections::HashMap;

/// Trait for reacting to raw input events from any device.
pub trait InputListener: Send {
    fn on_input(&mut self, event: &InputEvent);
}

/// Determines which kinds of events a listener wants to receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventFilter {
    All,
    AxisOnly,
    ButtonsOnly,
    Custom(fn(&InputEvent) -> bool), // Optional
}

/// Metadata-wrapped listener with filters and control flags.
struct ListenerEntry {
    listener: Box<dyn InputListener>,
    enabled: bool,
    filter: EventFilter,
    tag: Option<String>, // Optional device ID prefix or label
}

pub struct InputEventBus {
    next_id: u64,
    listeners: HashMap<u64, ListenerEntry>,
}

impl InputEventBus {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            listeners: HashMap::new(),
        }
    }

    /// Registers a listener with optional filtering and tag.
    pub fn add_listener(
        &mut self,
        listener: impl InputListener + 'static,
        filter: EventFilter,
        tag: Option<String>,
    ) -> u64 {
        let id = self.next_id;
        self.listeners.insert(
            id,
            ListenerEntry {
                listener: Box::new(listener),
                enabled: true,
                filter,
                tag,
            },
        );
        self.next_id += 1;
        id
    }

    /// Enables a previously registered listener.
    pub fn enable(&mut self, id: u64) {
        if let Some(entry) = self.listeners.get_mut(&id) {
            entry.enabled = true;
        }
    }

    /// Disables (mutes) a listener without removing it.
    pub fn disable(&mut self, id: u64) {
        if let Some(entry) = self.listeners.get_mut(&id) {
            entry.enabled = false;
        }
    }

    /// Unregisters a listener entirely.
    pub fn remove_listener(&mut self, id: u64) {
        self.listeners.remove(&id);
    }

    /// Emits one event to all active and matching listeners.
    fn emit(&mut self, event: &InputEvent) {
        for entry in self.listeners.values_mut() {
            if !entry.enabled {
                continue;
            }

            // If tagged, ensure this listener wants this event's device
            if let Some(ref wanted_id) = entry.tag {
                if event.device_id != *wanted_id {
                    continue;
                }
            }

            // Check event type filter
            let passes_filter = match entry.filter {
                EventFilter::All => true,
                EventFilter::AxisOnly => matches!(event.kind, InputKind::AxisMoved { .. }),
                EventFilter::ButtonsOnly => matches!(
                    event.kind,
                    InputKind::ButtonPressed { .. } | InputKind::ButtonReleased { .. }
                ),
                EventFilter::Custom(f) => f(event),
            };

            if passes_filter {
                entry.listener.on_input(event);
            }
        }
    }

    /// Emits a batch of events to matching listeners.
    pub fn emit_all(&mut self, events: &[InputEvent]) {
        for event in events {
            self.emit(event);
        }
    }
}
