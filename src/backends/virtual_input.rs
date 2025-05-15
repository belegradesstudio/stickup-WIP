use crate::{Device, InputEvent};

/// A software-emulated input device.
///
/// Useful for testing, scripting, or simulating physical input.
#[derive(Default)]
pub struct VirtualDevice {
    id: String,
    name: String,
    events: Vec<InputEvent>,
}

impl VirtualDevice {
    /// Creates a new virtual device with a custom ID and name.
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            events: Vec::new(),
        }
    }

    /// Queues a raw input event.
    pub fn feed(&mut self, event: InputEvent) {
        self.events.push(event);
    }

    /// Queues an axis movement event.
    pub fn set_axis(&mut self, axis: u8, value: f32) {
        self.feed(InputEvent::AxisMoved { axis, value });
    }

    /// Queues a button press.
    pub fn press_button(&mut self, button: u8) {
        self.feed(InputEvent::ButtonPressed { button });
    }

    /// Queues a button release.
    pub fn release_button(&mut self, button: u8) {
        self.feed(InputEvent::ButtonReleased { button });
    }
}

impl Device for VirtualDevice {
    /// Returns and clears all queued events.
    fn poll(&mut self) -> Vec<InputEvent> {
        let events = self.events.clone();
        self.events.clear();
        events
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> &str {
        &self.id
    }
}

/// Returns a single default virtual input device.
pub fn create_virtual_devices() -> Vec<Box<dyn Device>> {
    vec![Box::new(VirtualDevice::new("virtual:0", "Virtual Input 0"))]
}
