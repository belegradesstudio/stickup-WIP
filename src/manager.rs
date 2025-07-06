//! src/devices/manager.rs
use crate::devices::Device;
use crate::devices::InputEvent;
use crate::devices::InputKind;
use crate::devices::backends::hid::probe_devices;
use crate::devices::binding::DeviceState;
use crate::devices::eventbus::InputEventBus;
use std::collections::HashMap;

/// Central manager for all input devices and state tracking.
///
/// Automatically discovers devices via the enabled backends (`hid`, `virtual`).
/// Provides methods for polling, snapshotting, and binding resolution.
pub struct DeviceManager {
    devices: Vec<Box<dyn Device>>,
    snapshot_cache: HashMap<String, DeviceState>,
    pub eventbus: InputEventBus,
}

impl DeviceManager {
    /// Creates a new device manager and probes all available devices.
    ///
    /// Automatically includes HID and/or virtual devices based on enabled features.
    pub fn new() -> Self {
        let mut devices = Vec::new();
        {
            match hidapi::HidApi::new() {
                Ok(api) => {
                    let hid_devices = probe_devices(&api);
                    println!("Discovered {} HID device(s)", hid_devices.len());
                    devices.extend(hid_devices);
                }
                Err(e) => {
                    eprintln!("Failed to initialize HID API: {e}");
                }
            }
        }

        {
            let virtual_devices = crate::devices::backends::virtual_input::create_virtual_devices();
            println!("Loaded {} virtual device(s)", virtual_devices.len());
            devices.extend(virtual_devices);
        }

        Self {
            devices,
            snapshot_cache: HashMap::new(),
            eventbus: InputEventBus::new(),
        }
    }

    //// Adds a custom device (physical or virtual) to the manager.
    ///
    /// Can be used to inject game-specific or testing devices.
    pub fn add_device<D: Device + 'static>(&mut self, device: D) {
        self.devices.push(Box::new(device));
    }

    //// Polls all registered devices and returns raw input events.
    ///
    /// Does not affect internal state tracking — for stream-based usage.
    pub fn poll_all(&mut self) -> Vec<InputEvent> {
        let mut events = Vec::new();

        for device in self.devices.iter_mut() {
            let device_id = device.id().to_string();
            let raw_events = device.poll();

            for kind in raw_events {
                events.push(InputEvent {
                    device_id: device_id.clone(),
                    kind,
                });
            }
        }

        self.eventbus.emit_all(&events);
        events
    }

    /// Builds and caches a snapshot of current device states.
    ///
    /// Returns a mapping of `device_id` → [`DeviceState`], including axis and button values.
    /// This snapshot is reused during the current frame and cleared on the next `snapshot()` call.
    pub fn snapshot(&mut self) -> &HashMap<String, DeviceState> {
        self.snapshot_cache.clear();

        for device in self.devices.iter_mut() {
            let device_id = device.id().to_string();
            let mut state = DeviceState::default();

            let raw_events = device.poll();
            let wrapped_events: Vec<_> = raw_events
                .into_iter()
                .map(|kind| InputEvent {
                    device_id: device_id.clone(),
                    kind,
                })
                .collect();

            self.eventbus.emit_all(&wrapped_events);

            for event in wrapped_events {
                match event.kind {
                    InputKind::AxisMoved { axis, value } => {
                        state.axes.insert(axis.to_string(), value);
                    }
                    InputKind::ButtonPressed { button } => {
                        state.buttons.insert(button.to_string(), true);
                    }
                    InputKind::ButtonReleased { button } => {
                        state.buttons.insert(button.to_string(), false);
                    }
                }
            }

            self.snapshot_cache.insert(device_id, state);
        }

        &self.snapshot_cache
    }
    /// Retrieves the current value of an axis using a `"device_id.axis_name"` binding string.
    ///
    /// Returns `None` if the device or axis is not found.
    pub fn get_axis(&mut self, binding: &str) -> Option<f32> {
        if let Some((device_id, axis_id)) = binding.split_once('.') {
            let snapshot = self.snapshot();
            return snapshot.get(device_id)?.axes.get(axis_id).copied();
        }

        None
    }

    /// Checks whether a button is currently pressed using a `"device_id.button_name"` binding string.
    ///
    /// Returns `false` if the device or button is not found.
    pub fn is_pressed(&mut self, binding: &str) -> bool {
        if let Some((device_id, button_id)) = binding.split_once('.') {
            let snapshot = self.snapshot();
            return snapshot
                .get(device_id)
                .and_then(|state| state.buttons.get(button_id))
                .copied()
                .unwrap_or(false);
        }

        false
    }

    /// Returns an iterator over all currently registered devices.
    ///
    /// Useful for UI display, debugging, or device introspection.
    pub fn devices(&self) -> impl Iterator<Item = &Box<dyn Device>> {
        self.devices.iter()
    }
}
