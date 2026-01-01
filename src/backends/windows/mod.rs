#![cfg(target_os = "windows")]

//! Windows input backends.
//!
//! This module contains the Windows-specific implementations used by StickUp:
//! - **HID** discovery and device polling via `hidapi`
//! - **HIDP** report parsing (for consistent axis/button/hat events)
//! - **XInput** controller support
//! - **Raw Input** helpers for keyboard/mouse ingestion (WM_INPUT parsing)
//!
//! Most users should not interact with these modules directly. Prefer the high-level
//! [`Manager`](crate::manager::Manager) API:
//! - `Manager::discover()` for unified discovery
//! - `poll_events()` to receive deltas and update state
//! - `snapshot()` for per-tick state views
//!
//! The Raw Input parser is exposed to support host applications that own the Win32
//! message loop and want to forward WM_INPUT packets into StickUp.

pub mod hid_device;
pub mod hid_discovery;
pub mod hidp_parser;
pub mod raw_input;
pub mod xinput_devices;

pub use hid_discovery::probe_devices;
pub use hid_discovery::probe_devices_with_debug;
