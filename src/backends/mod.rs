//! Input backends for StickUp.
//!
//! These modules provide implementations of the [`Device`] trait
//! for real hardware (HID) and virtual input devices.

#[cfg(feature = "hid")]
pub mod hid;

#[cfg(feature = "virtual")]
pub mod virtual_input;

#[cfg(feature = "hid")]
pub use hid::probe_devices;

#[cfg(feature = "virtual")]
pub use virtual_input::create_virtual_devices;
