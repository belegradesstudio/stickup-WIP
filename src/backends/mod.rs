//! Input backends for `stickup`.
//!
//! Implementations of [`Device`](crate::device::Device) for platform-specific
//! input sources.
//!
//! # Feature flags
//! - **`hid`** — enables the Windows HID/XInput backend (default in this build).
//! - **`virtual`** — reserved (no virtual-device backend is currently wired up).
//!
//! StickUp reads input devices; it does not create virtual devices (vJoy/uinput).

use crate::device::Device;

#[cfg(all(feature = "hid", target_os = "windows"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "hid", target_os = "windows"))))]
pub mod windows;

/// Unified discovery across enabled backends.
///
/// Currently this returns HID/XInput devices on Windows when `hid` is enabled.
pub fn probe_devices() -> Vec<Box<dyn Device>> {
    let mut out: Vec<Box<dyn Device>> = Vec::new();

    #[cfg(all(feature = "hid", target_os = "windows"))]
    {
        use crate::backends::windows::probe_devices as win_probe;
        if let Ok(api) = hidapi::HidApi::new() {
            out.extend(win_probe(&api));
        }
    }

    out
}
