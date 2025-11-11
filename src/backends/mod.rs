//! Input backends for `stickup`.
//!
//! Implementations of [`Device`](crate::device::Device) for real hardware (HID)
//! and software-emulated (virtual) devices.
//!
//! # Feature flags
//! - **`hid`** — enables the HID backend (default).
//! - **`virtual`** — enables the virtual device backend (default).
//!
//! # Re-exports
//! For convenience this module re-exports:
//! - [`probe_devices`] — enumerate HID devices (requires `hid`).
//! - [`create_virtual_devices`] — create one default virtual device (requires `virtual`).

#[cfg(feature = "hid")]
#[cfg_attr(docsrs, doc(cfg(feature = "hid")))]
pub mod hid;

#[cfg(feature = "virtual")]
#[cfg_attr(docsrs, doc(cfg(feature = "virtual")))]
pub mod virtual_input;

pub mod win_hidp;

#[cfg(feature = "hid")]
#[doc(inline)]
#[cfg_attr(docsrs, doc(cfg(feature = "hid")))]
pub use hid::probe_devices;

#[cfg(feature = "virtual")]
#[doc(inline)]
#[cfg_attr(docsrs, doc(cfg(feature = "virtual")))]
pub use virtual_input::create_virtual_devices;
