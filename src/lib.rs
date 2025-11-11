//! StickUp — Modular input device manager for Rust.
//!
//! Provides a unified interface for polling HID and virtual input devices,
//! resolving bindings, and generating normalized control states.
//!
//! # Features
//! - **`hid`** — enable the HID backend (default).
//! - **`virtual`** — enable a software-emulated input device (default).
//!
//!
//! # Windows HIDP parser (Windows only)
//! On Windows, StickUp will first attempt a descriptor-driven parser based on the
//! OS HID Parser (“HIDP”). This parser:
//! - enumerates **buttons** via HID usages (emitting press/release edges),
//! - reads **axes** with their true logical ranges (normalized to `[-1.0, 1.0]`), and
//! - standardizes **hat switches** to slot values `-1` (neutral) or `0..7` (N..NW).
//!
//! If HIDP initialization fails for a device, StickUp **falls back** to a generic,
//! little-endian 16-bit stride parser that still yields usable axis motion.
//!
//! > Tip: You don’t need to opt into this manually. When the `hid` feature is
//! > enabled, the HID backend will try HIDP automatically and only fall back
//! > if necessary.
//!
//! # The `prelude`
//! For app crates, `stickup::prelude::*` re-exports the most common types:
//! device/manager traits, events & event bus, metadata/snapshots, and bindings.
//! It’s purely ergonomic — you can also import items from their modules directly.
//!
//!
//! # Getting started
//! ```ignore
//! use stickup::{DeviceManager, Snapshot};
//!
//! let mut mgr = DeviceManager::new();
//! let _events = mgr.poll_all();   // dispatch to the bus
//! let snap: Snapshot = mgr.snapshot();
//! for (id, state) in snap.iter() {
//!     println!("{id} X={}", state.get_axis("X"));
//! }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod backends;
pub mod binding;
pub mod device;
pub mod event;
pub mod eventbus;
pub mod filtered_listener;
pub mod logger;
pub mod manager;
pub mod metadata;
pub mod snapshot;

// ---- Re-exports (convenience) ----

pub use binding::*;
pub use device::*;
pub use event::*;
pub use eventbus::*;
pub use manager::*;
pub use metadata::*;
pub use snapshot::Snapshot;

// Optional: ergonomic import bundle for app crates.
pub mod prelude {
    // Bindings & transforms
    pub use crate::binding::{
        AxisCurve, AxisTransform, BindingOutput, BindingProfile, BindingRule, ControlPath,
        ControlPath2D, ControlType, DeviceState,
    };

    // Core device trait & manager
    pub use crate::device::Device;
    pub use crate::manager::DeviceManager;

    // Events & event bus
    pub use crate::event::{InputEvent, InputKind};
    pub use crate::eventbus::{EventFilter, InputEventBus, InputListener};

    // Metadata & snapshots
    pub use crate::metadata::DeviceMeta;
    pub use crate::snapshot::Snapshot;

    // Optional: logger convenience (nice for quick demos)
    pub use crate::logger::Logger;

    // Optional (only if you want it public):
    #[cfg(feature = "hid")]
    #[cfg_attr(docsrs, doc(cfg(feature = "hid")))]
    pub use crate::backends::hid::probe_devices;
}
