#[cfg(feature = "hid")]
use crate::backends::hid::probe_devices;
use crate::binding::DeviceState;
use crate::{Device, InputEvent};
use std::collections::HashMap;

/// Manages input devices and provides polling/snapshot interfaces.
///
/// Devices are discovered automatically via enabled features (`hid`, `virtual`).
pub struct DeviceManager {
    devices: Vec<Box<dyn Device>>,
}

impl DeviceManager {
    /// Creates a new manager and registers available devices.
    pub fn new() -> Self {
        let mut manager = Self { devices: vec![] };

        #[cfg(feature = "hid")]
        {
            if let Ok(api) = hidapi::HidApi::new() {
                let hid_devices = probe_devices(&api);
                println!("Discovered {} HID device(s)", hid_devices.len());
                manager.devices.extend(hid_devices);
            } else {
                eprintln!("Failed to initialize HID API");
            }
        }

        #[cfg(feature = "virtual")]
        {
            let virtual_devices = crate::backends::virtual_input::create_virtual_devices();
            println!("Loaded {} virtual device(s)", virtual_devices.len());
            manager.devices.extend(virtual_devices);
        }

        manager
    }

    /// Adds a custom device to the manager.
    pub fn add_device<D: Device + 'static>(&mut self, device: D) {
        self.devices.push(Box::new(device));
    }

    /// Polls all devices and collects their current input events.
    pub fn poll_all(&mut self) -> Vec<InputEvent> {
        let mut events = Vec::new();
        for device in self.devices.iter_mut() {
            events.extend(device.poll());
        }
        events
    }

    /// Returns a snapshot of all device states.
    ///
    /// The result is a map of device ID → `DeviceState`.
    pub fn snapshot(&mut self) -> HashMap<String, DeviceState> {
        let mut map = HashMap::new();

        for device in &mut self.devices {
            let device_id = device.id().to_string();
            let mut state = DeviceState::default();

            for event in device.poll() {
                match event {
                    InputEvent::AxisMoved { axis, value } => {
                        state.axes.insert(axis.to_string(), value);
                    }
                    InputEvent::ButtonPressed { button } => {
                        state.buttons.insert(button.to_string(), true);
                    }
                    InputEvent::ButtonReleased { button } => {
                        state.buttons.insert(button.to_string(), false);
                    }
                }
            }

            map.insert(device_id, state);
        }

        map
    }
}
