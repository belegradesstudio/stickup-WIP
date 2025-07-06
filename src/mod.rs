//! StickUp — Modular input device manager for Rust.
//!
//! Provides a unified interface for polling HID and virtual input devices,
//! resolving bindings, and generating normalized control states.

pub mod backends;
pub mod binding;
pub mod device;
pub mod event;
pub mod eventbus;
pub mod filtered_listener;
pub mod logger;
pub mod manager;

pub use binding::*;
pub use device::*;
pub use event::*;
pub use eventbus::*;
pub use manager::*;
