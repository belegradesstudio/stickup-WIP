#![cfg_attr(docsrs, feature(doc_cfg))]
//! `stickup`: a modular input device manager for reading controllers and input devices.
//!
//! StickUp focuses on **input ingestion** (reading devices). It is **not** a virtual
//! device generator (it does not create vJoy/uinput-style devices).
//!
//! # Platform support
//! - **Windows**: HID devices (via `hidapi` + HIDP parsing) and **XInput** controllers.
//! - Other platforms: backend support is currently limited / not implemented in this build.
//!
//! # Quick start
//! ```no_run
//! use stickup::Manager;
//! let mut mgr = Manager::discover().expect("discover devices");
//! loop {
//!     // Polling updates the internal per-device state and yields any deltas.
//!     for (id, ev) in mgr.poll_events() {
//!         println!("{id}: {ev:?}");
//!     }
//!
//!     // `snapshot()` is a view of the last polled state (owned clone).
//!     let snap = mgr.snapshot();
//!     // ...feed `snap` into bindings/UI, etc.
//!     drop(snap);
//! }
//! ```
//!
//! # Modules
//! - [`device`] — core device trait implemented by backends
//! - [`event`] — input events and channel descriptions
//! - [`binding`] — device-agnostic mapping/transforms
//! - [`metadata`] — device metadata struct
//! - [`snapshot`] — per-frame view for bindings/UI (owned)
//! - [`backends`] — platform-specific implementations
//! - [`Manager`] — high-level cross-device API
//!
//! # Feature flags
//! - **`hid`** — enables the Windows HID/XInput backend (default in this build).
//! - **`virtual`** — reserved (no virtual-device backend is currently wired up).
//!
//! # Windows Raw Input
//! On Windows, you can also feed keyboard/mouse Raw Input into the manager using:
//! - [`Manager::handle_wm_input`](crate::manager::Manager::handle_wm_input)
//! - [`Manager::handle_raw_input_bytes`](crate::manager::Manager::handle_raw_input_bytes)
//!
//! ## Threading
//! `Manager` owns live device handles and is intended to live on **one thread**.
//! If multiple threads need to interact, route calls through a message-passing
//! façade on a single “bridge” thread that owns the `Manager`. This avoids
//! duplicate discoveries and honors backend thread-affinity.

pub mod backends;
pub mod binding;
pub mod device;
pub mod event;
pub mod manager;
pub mod metadata;
pub mod snapshot;

/// Error and Result types for the crate.
pub mod error {
    //! Error and result types used across StickUp.
    //!
    //! Most public APIs that can fail return [`Result`](Result), notably:
    //! - [`Manager::discover`](crate::manager::Manager::discover)
    //!
    //! Many operations are infallible after discovery (polling, snapshots), so this module
    //! stays small by design.

    /// Crate-wide error type.
    ///
    /// StickUp keeps errors intentionally lightweight. Backend-specific details may be
    /// mapped into [`Error::Other`] as a human-readable string.
    #[derive(thiserror::Error, Debug)]
    pub enum Error {
        /// The requested backend is not available for this build/OS.
        ///
        /// Typical causes:
        /// - building on a non-supported platform
        /// - the relevant Cargo feature is disabled
        #[error("HID backend not available on this platform/build")]
        BackendUnavailable,

        /// Opaque backend error surfaced as a message.
        ///
        /// This is used when a backend wants to report a failure without exposing
        /// platform-specific error types in the public API.
        #[error("{0}")]
        Other(String),
    }

    /// Convenient crate-wide result alias.
    pub type Result<T> = core::result::Result<T, Error>;
}

pub use error::{Error, Result};
pub use manager::Manager;

// ---- Re-exports (convenience) ----
pub use binding::*;
pub use event::*;
pub use metadata::DeviceMeta;
pub use snapshot::Snapshot;

// A tiny prelude for downstreams.
pub mod prelude {
    pub use crate::binding::{
        AxisCurve, AxisTransform, BindingOutput, BindingProfile, BindingRule, ControlPath,
        ControlPath2D, ControlType, DeviceState,
    };
    pub use crate::event::{ChannelDesc, ChannelKind, InputEvent, InputKind};
    pub use crate::manager::{Manager, RescanReport};
    pub use crate::metadata::DeviceMeta;
    pub use crate::snapshot::Snapshot;
}

// Internal glue: a single probe function all backends conform to.
#[doc(hidden)]
pub(crate) fn _probe_devices_internal() -> Vec<Box<dyn device::Device>> {
    backends::probe_devices()
}
