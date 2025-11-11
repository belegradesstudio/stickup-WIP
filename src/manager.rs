//! Central device manager: discovery, polling, snapshots, and event dispatch.
//!
//! `DeviceManager` owns all active input devices (HID + virtual, feature-gated),
//! provides a single entry point for **polling** and **snapshotting**, and emits
//! events through an embedded [`InputEventBus`](crate::eventbus::InputEventBus).
//!
//! # Features
//! - **`hid`**: probes physical devices via `hidapi` and wraps them as [`Device`]s.
//! - **`virtual`**: adds a default software-emulated device for testing/scripts.
//!
//! # Design
//! - **Polling model:** single-threaded; call [`poll_all`] or [`snapshot`] once per tick.
//! - **Event flow:** device → [`Device::poll`] → [`InputEvent`] → [`eventbus`](Self::eventbus).
//! - **State:** `snapshot()` maintains a sticky `last_state` per device and returns an owned
//!   `Snapshot` for the frame.
//!
//! # Examples
//! Poll devices then derive a frame snapshot:
//! ```ignore
//! use stickup::devices::manager::DeviceManager;
//!
//! let mut mgr = DeviceManager::new();
//! mgr.poll_all();                 // emit events to the bus
//! let snap = mgr.snapshot();      // owned copy of current states
//! for (dev_id, state) in snap.iter() {
//!     let x = state.axes.get("X").copied().unwrap_or(0.0);
//!     println!("{dev_id}: X={x} buttons={}", state.buttons.len());
//! }
//! ```
//!
//! Access a bound axis by `"device_id.axis_name"`:
//! ```ignore
//! let v = mgr.get_axis("046d:c216:SER0.X").unwrap_or(0.0);
//! ```
//!
//! Check a button’s pressed state by `"device_id.button_name"`:
//! ```ignore
//! let pressed = mgr.is_pressed("virtual:0.Trigger");
//! ```

use crate::backends::hid::probe_devices;
use crate::binding::DeviceState; // NOTE: adjust to your actual module path if needed.
use crate::device::Device;
use crate::event::{InputEvent, InputKind};
use crate::eventbus::InputEventBus;
use crate::snapshot::Snapshot;
use crate::DeviceMeta;
use std::collections::HashMap;
use std::collections::HashSet;

#[cfg(feature = "hid")]
use hidapi::HidApi;
/// Central manager for all input devices and state tracking.
///
/// Automatically discovers devices via the enabled backends (`hid`, `virtual`).
/// Provides methods for polling, snapshotting, and binding resolution.
pub struct DeviceManager {
    devices: Vec<Box<dyn Device>>,
    last_state: HashMap<String, DeviceState>,
    /// Embedded single-threaded event bus; listeners can subscribe here.
    pub eventbus: InputEventBus,
}

/// Result of a hotplug rescan: which devices were added/removed by `id()`.
#[derive(Debug, Default, Clone)]
pub struct RescanReport {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl DeviceManager {
    /// Create a new device manager and probe all available devices.
    ///
    /// - **HID** discovery uses `hidapi` (requires the `hid` feature).
    /// - **Virtual** devices are created if the `virtual` feature is enabled.
    pub fn new() -> Self {
        let mut devices = Vec::new();

        // HID discovery
        #[cfg(feature = "hid")]
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

        // Virtual device(s)
        #[cfg(feature = "virtual")]
        {
            let virtual_devices = crate::backends::virtual_input::create_virtual_devices();
            println!("Loaded {} virtual device(s)", virtual_devices.len());
            devices.extend(virtual_devices);
        }

        Self {
            devices,
            last_state: HashMap::new(),
            eventbus: InputEventBus::new(),
        }
    }

    /// Add a custom device (physical or virtual) to the manager.
    ///
    /// Useful for injecting game-specific or testing devices.
    pub fn add_device<D: Device + 'static>(&mut self, device: D) {
        self.devices.push(Box::new(device));
    }

    /// Poll all registered devices and return raw input events.
    ///
    /// This **does not** update `last_state`; it’s suitable for stream-based usage.
    /// For stateful, per-frame values, use [`snapshot`](Self::snapshot).
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

    /// Return current live device metadata as `(device_id, meta)` pairs.
    pub fn metadata(&self) -> Vec<(String, DeviceMeta)> {
        self.devices
            .iter()
            .map(|d| (d.id().to_string(), d.metadata()))
            .collect()
    }

    /// Build an **owned** snapshot of current device states.
    ///
    /// - Emits events while integrating deltas and updating sticky `last_state`.
    /// - Intended to be called once per frame/tick by higher layers.
    pub fn snapshot(&mut self) -> Snapshot {
        let mut frame_map: HashMap<String, DeviceState> = HashMap::new();

        for device in self.devices.iter_mut() {
            let device_id = device.id().to_string();

            // start from previous (sticky) state
            let mut state = self.last_state.get(&device_id).cloned().unwrap_or_default();

            // poll → wrap → emit
            let raw_events = device.poll();
            let wrapped: Vec<_> = raw_events
                .into_iter()
                .map(|kind| InputEvent {
                    device_id: device_id.clone(),
                    kind,
                })
                .collect();
            self.eventbus.emit_all(&wrapped);

            // apply deltas onto sticky state
            for e in wrapped {
                match e.kind {
                    InputKind::AxisMoved { axis, value } => {
                        state.axes.insert(axis.to_string(), value);
                    }
                    InputKind::ButtonPressed { button } => {
                        state.buttons.insert(button.to_string(), true);
                    }
                    InputKind::ButtonReleased { button } => {
                        state.buttons.insert(button.to_string(), false);
                    }
                    InputKind::HatChanged { hat, value } => {
                        apply_hat_to_state(&mut state, hat, value);
                    }
                }
            }

            // persist sticky + build frame view
            self.last_state.insert(device_id.clone(), state.clone());
            frame_map.insert(device_id, state);
        }

        Snapshot(frame_map)
    }

    /// Get the current value of an axis using a `"device_id.axis_name"` binding string.
    ///
    /// Returns `None` if the device or axis is not found.
    pub fn get_axis(&mut self, binding: &str) -> Option<f32> {
        if let Some((device_id, axis_id)) = binding.split_once('.') {
            let snapshot = self.snapshot();
            return snapshot.get(device_id)?.axes.get(axis_id).copied();
        }
        None
    }

    /// Check whether a button is pressed via `"device_id.button_name"`.
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

    /// Iterate all currently registered devices (by trait object).
    ///
    /// Useful for UI display, debugging, or device introspection.
    pub fn devices(&self) -> impl Iterator<Item = &Box<dyn Device>> {
        self.devices.iter()
    }

    /// Re-probes physical HID devices and updates the manager set.
    ///
    /// - Virtual devices are preserved.
    /// - Physical devices are diffed by `Device::id()`.
    /// - `last_state` entries for removed devices are purged.
    /// - Returns lists of `added` and `removed` device IDs.
    pub fn rescan(&mut self) -> RescanReport {
        // Partition current devices into virtual vs physical by metadata().bus
        let mut virtual_keep: Vec<Box<dyn Device>> = Vec::new();
        let mut current_phys_ids: HashSet<String> = HashSet::new();

        for dev in self.devices.drain(..) {
            let bus = dev.metadata().bus.clone();
            if bus.as_deref() == Some("virtual") {
                virtual_keep.push(dev);
            } else {
                current_phys_ids.insert(dev.id().to_string());
                // drop old physical instance; we’ll re-add fresh ones
            }
        }

        // Probe fresh physical set (if HID feature present)
        #[cfg(feature = "hid")]
        let new_phys: Vec<Box<dyn Device>> = {
            let mut out = Vec::new();
            if let Ok(api) = HidApi::new() {
                out = probe_devices(&api);
            }
            out
        };
        #[cfg(not(feature = "hid"))]
        let new_phys: Vec<Box<dyn Device>> = Vec::new();

        let new_phys_ids: HashSet<String> = new_phys.iter().map(|d| d.id().to_string()).collect();

        // Compute diffs
        let added: Vec<String> = new_phys_ids
            .difference(&current_phys_ids)
            .cloned()
            .collect();
        let removed: Vec<String> = current_phys_ids
            .difference(&new_phys_ids)
            .cloned()
            .collect();

        // Purge last_state of removed devices
        for id in &removed {
            self.last_state.remove(id);
        }

        // Reassemble device list: keep virtuals, append new physicals
        self.devices = virtual_keep;
        self.devices.extend(new_phys.into_iter());

        RescanReport { added, removed }
    }
}

/// Integrate a hat event into [`DeviceState`] as both axes and directional buttons.
///
/// Writes two **synthetic axes** (`"hatN_x"`, `"hatN_y"`) with values in `{-1, 0, 1}`
/// and four **directional buttons** (`"hatN_up/right/down/left"`). Diagonals set both
/// relevant buttons to `true`.
///
/// Neutral encodings recognized: `-1`, `8`, `15`, `255`.
fn apply_hat_to_state(state: &mut DeviceState, hat: u16, value: i16) {
    // Always record the raw hat value in the structured `hats` map.
    state.hats.insert(format!("hat{hat}"), value);
    let is_neutral = matches!(value, -1 | 8 | 15 | 255);

    // 0..7 clockwise: 0=Up, 1=Up-Right, 2=Right, 3=Down-Right, 4=Down, 5=Down-Left, 6=Left, 7=Up-Left
    let (x, y, up, right, down, left) = if is_neutral {
        (0, 0, false, false, false, false)
    } else {
        match value as u16 {
            0 => (0, 1, true, false, false, false),  // Up
            1 => (1, 1, true, true, false, false),   // Up-Right
            2 => (1, 0, false, true, false, false),  // Right
            3 => (1, -1, false, true, true, false),  // Down-Right
            4 => (0, -1, false, false, true, false), // Down
            5 => (-1, -1, false, false, true, true), // Down-Left
            6 => (-1, 0, false, false, false, true), // Left
            7 => (-1, 1, true, false, false, true),  // Up-Left
            _ => (0, 0, false, false, false, false), // Unknown → neutral
        }
    };

    // Synthetic axes (digital grid; diagonals keep magnitude 1 on each axis)
    state.axes.insert(format!("hat{hat}_x"), x as f32);
    state.axes.insert(format!("hat{hat}_y"), y as f32);

    // Directional buttons (diagonals set two to true)
    let base = format!("hat{hat}_");
    state.buttons.insert(format!("{base}up"), up);
    state.buttons.insert(format!("{base}right"), right);
    state.buttons.insert(format!("{base}down"), down);
    state.buttons.insert(format!("{base}left"), left);
}
