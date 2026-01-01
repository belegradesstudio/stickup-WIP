//! Device metadata snapshot.
//!
//! [`DeviceMeta`] is a lightweight, cloneable description of a device suitable
//! for UI display, logging, and persistence. Backends populate what they know;
//! unknown fields remain `None`.
//!
//! # Conventions
//! - `bus` is a short, human-readable bus hint like `"usb"`, `"bluetooth"`, or `"rawinput"`.
//! - `product_string` should be a friendly, user-facing name when available.
//! - `path` is an OS/topology path (opaque string) useful for diagnostics.
//! - HID-specific fields (`usage_page`, `usage`, `interface_number`) are filled
//!   when provided by the platform.
//!
//! ## Persistence notes
//! - `vid`/`pid` and `serial_number` (when present) are generally stable and useful for re-identification.
//! - `path` is platform-specific and may change across ports, drivers, and reconnects; treat it as
//!   diagnostic first, identity second.
//!
//! # Example
//! ```no_run
//! use stickup::Manager;
//!
//! let mgr = Manager::discover().expect("discover devices");
//! for info in mgr.devices() {
//!     println!("{}: {:?}", info, info.meta);
//! }
//! ```

use serde::{Deserialize, Serialize};

/// Snapshot of metadata describing a single device.
///
/// All fields are optional; populate what is known on the current platform.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceMeta {
    /// High-level bus classification (e.g., `"usb"`, `"bluetooth"`, `"rawinput"`).
    pub bus: Option<String>,

    /// USB Vendor ID (VID), if known.
    pub vid: Option<u16>,

    /// USB Product ID (PID), if known.
    pub pid: Option<u16>,

    /// Human-readable product name from the driver/firmware.
    ///
    /// Backends should prefer the OS-reported product string when present.
    pub product_string: Option<String>,

    /// Device serial number supplied by firmware/OS, if present.
    ///
    /// On USB, this usually maps to the iSerialNumber string.
    pub serial_number: Option<String>,

    /// HID interface index (platform-reported).
    ///
    /// Some stacks use `-1` to mean “not applicable”.
    pub interface_number: Option<i32>,

    /// HID Usage Page (e.g., `0x01` for Generic Desktop), if known.
    pub usage_page: Option<u16>,

    /// HID Usage within the page (e.g., `0x04` Joystick, `0x05` Gamepad), if known.
    pub usage: Option<u16>,

    /// OS/topological path to the device.
    ///
    /// Useful for diagnostics; format is platform-specific and should be treated as opaque.
    pub path: Option<String>,

    /// Windows-only: container identifier (DEVPKEY_Device_ContainerId), if known.
    ///
    /// Identifies a logical container that may group related interfaces.
    pub container_id: Option<String>,
}
