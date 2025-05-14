use crate::{Device, InputEvent};

#[derive(Default)]
pub struct VirtualDevice {
    id: String,
    name: String,
    events: Vec<InputEvent>,
}

impl VirtualDevice {
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            events: Vec::new(),
        }
    }

    /// Inject a raw input event into the virtual device.
    pub fn feed(&mut self, event: InputEvent) {
        self.events.push(event);
    }

    /// Convenience method to set an axis value.
    pub fn set_axis(&mut self, axis: u8, value: f32) {
        self.feed(InputEvent::AxisMoved { axis, value });
    }

    pub fn press_button(&mut self, button: u8) {
        self.feed(InputEvent::ButtonPressed { button });
    }

    pub fn release_button(&mut self, button: u8) {
        self.feed(InputEvent::ButtonReleased { button });
    }
}

impl Device for VirtualDevice {
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

pub fn create_virtual_devices() -> Vec<Box<dyn Device>> {
    vec![Box::new(VirtualDevice::new("virtual:0", "Virtual Input 0"))]
}
