//! Per-frame snapshot of device states.
//!
//! [`Snapshot`] is an **owned**, read-only view of all device states at a point
//! in time (typically “this frame”). It’s produced by higher-level code like
//! [`Manager::snapshot`](crate::manager::Manager::snapshot) and is cheap to clone
//! for fan-out to multiple consumers.
//!
//! # Semantics
//! - Keys are `device_id` strings (same format as [`Device::id`](crate::device::Device::id)).
//! - Values are [`DeviceState`](crate::binding::DeviceState) maps of axis/button/hat values.
//! - A snapshot is **immutable**. To refresh, call `poll_events()` and then request a new snapshot.
//! - `Snapshot` does **not** poll devices; it reflects the manager’s last-known state.
//!
//! # Examples
//! Iterate devices and read values:
//! ```no_run
//! use stickup::Snapshot;
//!
//! fn print_axes(snap: &Snapshot) {
//!     for (dev, state) in snap.iter() {
//!         let x = state.get_axis("X");
//!         let y = state.get_axis("Y");
//!         println!("{dev}: X={x:.2} Y={y:.2} pressed_trigger={}",
//!                  state.get_button("Trigger"));
//!     }
//! }
//! ```
//!
//! Extract and take ownership of the inner map:
//! ```ignore
//! let map = snap.clone().into_inner(); // HashMap<String, DeviceState>
//! ```

use crate::binding::DeviceState;
use std::collections::HashMap;

/// Owned snapshot of current device states (`device_id → DeviceState`).
///
/// Cloning is inexpensive for typical setups and useful for per-tick fan-out.
#[derive(Clone, Debug, Default)]
pub struct Snapshot(pub HashMap<String, DeviceState>);

impl Snapshot {
    /// Get the state for a specific `device_id`.
    #[inline]
    pub fn get(&self, device_id: &str) -> Option<&DeviceState> {
        self.0.get(device_id)
    }

    /// Iterate `(device_id, state)` pairs.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&String, &DeviceState)> {
        self.0.iter()
    }

    /// Consume the snapshot and return the inner map.
    #[inline]
    pub fn into_inner(self) -> HashMap<String, DeviceState> {
        self.0
    }
}
