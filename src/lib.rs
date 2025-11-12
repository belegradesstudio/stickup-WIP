#![cfg_attr(docsrs, feature(doc_cfg))]
//! `stickup`: a modular input device abstraction with HID and virtual backends.
//!
//! # Quick start
//! ```no_run
//! use stickup::{Manager, prelude::*};
//! let mut mgr = Manager::discover().expect("discover devices");
//! for (id, ev) in mgr.poll_events() {
//!     println!("{id}: {ev:?}");
//! }
//! ```
//!
//! # Modules
//! - [`device`] — core device trait implemented by backends
//! - [`event`] — input events and channel descriptions
//! - [`binding`] — device-agnostic mapping/transforms
//! - [`metadata`] — device metadata struct
//! - [`snapshot`] — per-frame view for bindings/UI
//! - [`backends`] — platform-specific implementations
//! - [`Manager`] — high-level cross-device API
//!
//! Enable features in `Cargo.toml`: `hid` (default), `virtual` (default).

pub mod backends;
pub mod binding;
pub mod device;
pub mod event;
pub mod manager;
pub mod metadata;
pub mod snapshot;

/// Error and Result types for the crate.
pub mod error {
    /// Crate-wide error type.
    #[derive(thiserror::Error, Debug)]
    pub enum Error {
        #[error("HID backend not available on this platform/build")]
        BackendUnavailable,
        #[error("{0}")]
        Other(String),
    }
    /// Convenient alias.
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
