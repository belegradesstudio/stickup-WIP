//! Input backends for StickUp.
//!
//! These modules provide implementations of the [`Device`] trait
//! for real hardware (HID) and virtual input devices.

pub mod hid;

pub mod virtual_input;

pub use hid::probe_devices;

pub use virtual_input::create_virtual_devices;
