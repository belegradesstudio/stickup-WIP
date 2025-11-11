//! Virtual input backend for `stickup`.
//!
//! This module provides a software-emulated device that implements the
//! [`Device`](crate::device::Device) trait. It’s useful for tests, demos,
//! scripted input, and environments without physical hardware.
//!
//! - **Feature gate:** enabled by the crate feature `virtual`.
//! - **Backend:** pure software; no external dependencies.
//!
//! # Overview
//! - [`VirtualDevice`]: a queue-backed, software-emulated input device.
//! - [`create_virtual_devices`]: convenience helper returning a single default
//!   virtual device as a boxed `dyn Device`.
//!
//! # Semantics
//! - Calling [`VirtualDevice::feed`] (or helpers like [`set_axis`](VirtualDevice::set_axis))
//!   enqueues events.
//! - [`Device::poll`](crate::device::Device::poll) returns **all** queued events
//!   and clears the queue (FIFO).
//! - Identity is stable and human-readable (`id` like `"virtual:0"`).
//!
//! # Examples
//! Basic usage (enqueue, then poll):
//! ```ignore
//! use stickup::devices::backends::virtual_input::VirtualDevice;
//! use stickup::device::Device;
//! use stickup::event::InputKind;
//!
//! let mut v = VirtualDevice::new("virtual:demo", "Scripted Device");
//! v.set_axis(0, 0.25);
//! v.press_button(1);
//! v.release_button(1);
//!
//! let events = v.poll(); // drains queue
//! assert!(events.iter().any(|e| matches!(e, InputKind::AxisMoved { axis: 0, .. })));
//! ```
//!
//! ## API Notes
//! - **Types:** [`VirtualDevice`]
//! - **Functions:** [`create_virtual_devices`]
//! - **Behavior:** queue-based; `poll()` drains and clears; `metadata()` advertises
//!   `bus = "virtual"` and uses the device `id` as a stable `path`.
////! src/devices/backends/virtual_input.rs

use crate::device::Device;
use crate::event::InputKind;
use crate::metadata::DeviceMeta;

/// A software-emulated input device.
///
/// Useful for testing, scripting, or simulating physical input.
///
/// Events are queued via [`feed`](Self::feed) (or the convenience methods) and
/// drained on [`Device::poll`].
#[derive(Default)]
pub struct VirtualDevice {
    id: String,
    name: String,
    events: Vec<InputKind>,
}

impl VirtualDevice {
    /// Creates a new virtual device with a custom ID and name.
    ///
    /// The `id` should be stable and unique within your process (e.g., `"virtual:0"`).
    ///
    /// # Example
    /// ```ignore
    /// let v = stickup::devices::backends::virtual_input::VirtualDevice::new("virtual:0", "Virtual Input 0");
    /// ```
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            events: Vec::new(),
        }
    }

    /// Queues a raw input event.
    ///
    /// Prefer using the convenience helpers when possible.
    ///
    /// # Example
    /// ```ignore
    /// use stickup::event::InputKind;
    /// let mut v = stickup::devices::backends::virtual_input::VirtualDevice::new("virtual:0", "V");
    /// v.feed(InputKind::ButtonPressed { button: 7 });
    /// ```
    pub fn feed(&mut self, event: InputKind) {
        self.events.push(event);
    }

    /// Queues an axis movement event.
    ///
    /// `value` should be normalized to `[-1.0, 1.0]` for consistency with physical devices.
    ///
    /// # Example
    /// ```ignore
    /// let mut v = stickup::devices::backends::virtual_input::VirtualDevice::new("virtual:0", "V");
    /// v.set_axis(2, -0.5);
    /// ```
    pub fn set_axis(&mut self, axis: u16, value: f32) {
        self.feed(InputKind::AxisMoved { axis, value });
    }

    /// Queues a button press.
    ///
    /// # Example
    /// ```ignore
    /// let mut v = stickup::devices::backends::virtual_input::VirtualDevice::new("virtual:0", "V");
    /// v.press_button(0);
    /// ```
    pub fn press_button(&mut self, button: u16) {
        self.feed(InputKind::ButtonPressed { button });
    }

    /// Queues a button release.
    ///
    /// # Example
    /// ```ignore
    /// let mut v = stickup::devices::backends::virtual_input::VirtualDevice::new("virtual:0", "V");
    /// v.release_button(0);
    /// ```
    pub fn release_button(&mut self, button: u16) {
        self.feed(InputKind::ButtonReleased { button });
    }
}

impl Device for VirtualDevice {
    /// Returns a batch of all queued events and clears the internal queue.
    ///
    /// Subsequent calls return only events queued after the previous `poll()`.
    fn poll(&mut self) -> Vec<InputKind> {
        let events = self.events.clone();
        self.events.clear();
        events
    }

    /// Human-readable device name.
    fn name(&self) -> &str {
        &self.name
    }

    /// Stable identifier string for this virtual device (e.g., `"virtual:0"`).
    fn id(&self) -> &str {
        &self.id
    }

    /// Returns metadata describing this virtual device.
    ///
    /// - `bus = "virtual"`
    /// - `product_string = name`
    /// - `path = id` (stable)
    fn metadata(&self) -> DeviceMeta {
        // Provide stable, descriptive identity for virtual devices.
        // `id` already looks like "virtual:0" — reuse it as a stable path.
        DeviceMeta {
            bus: Some("virtual".into()),
            vid: None,
            pid: None,
            product_string: Some(self.name.clone()),
            serial_number: None,
            interface_number: None,
            usage_page: None,
            usage: None,
            path: Some(self.id.clone()),
            container_id: None,
        }
    }
}

/// Returns a single default virtual input device.
///
/// Useful for quick-start scenarios where you want one software device without
/// constructing it manually.
///
/// # Example
/// ```ignore
/// use stickup::devices::backends::virtual_input::create_virtual_devices;
/// use stickup::device::Device;
///
/// let mut devices = create_virtual_devices();
/// let events = devices[0].poll(); // initially empty
/// ```
pub fn create_virtual_devices() -> Vec<Box<dyn Device>> {
    vec![Box::new(VirtualDevice::new("virtual:0", "Virtual Input 0"))]
}
