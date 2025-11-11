//! Input event types.
//!
//! StickUp represents device input as discrete, timestamp-free events produced by
//! polling backends or injected by virtual sources.
//!
//! # Overview
//! - [`InputEvent`]: device-scoped wrapper around an [`InputKind`].
//! - [`InputKind`]: concrete event variants (axis, button, hat).
//!
//! # Conventions
//! - Axes are normalized to `[-1.0, 1.0]` by convention.
//! - Button identifiers and axis indices are device-local (opaque `u16`).
//! - Hats may be reported as **8-way slots** (`-1` neutral, `0..=7`) *or*
//!   **degrees**; consumers should handle either (see docs on [`HatChanged`]).
//!
//! # Example
//! ```ignore
//! use stickup::devices::event::{InputEvent, InputKind};
//!
//! let e = InputEvent {
//!     device_id: "js0".into(),
//!     kind: InputKind::AxisMoved { axis: 0, value: -0.25 },
//! };
//!
//! match e.kind {
//!     InputKind::AxisMoved { axis, value } => { /* handle axis */ }
//!     InputKind::ButtonPressed { button } => { /* handle press */ }
//!     InputKind::ButtonReleased { button } => { /* handle release */ }
//!     InputKind::HatChanged { hat, value } => { /* handle POV */ }
//! }
//! ```

/// A discrete input event from a device (polled or injected).
#[derive(Debug, Clone)]
pub struct InputEvent {
    /// Stable ID of the device that produced this event.
    pub device_id: String,
    /// Specific input change.
    pub kind: InputKind,
}

/// Specific kinds of input changes.
///
/// Producers should adhere to the normalization guidance in each variant’s docs.
/// Consumers should be robust to absent channels (e.g., a device without hats).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputKind {
    /// Continuous axis movement.
    ///
    /// - `axis`: device-local axis index.
    /// - `value`: normalized to `[-1.0, 1.0]` by convention (left/down = negative; right/up = positive).
    AxisMoved { axis: u16, value: f32 },

    /// Button press (edge).
    ///
    /// - `button`: device-local button index.
    ButtonPressed { button: u16 },

    /// Button release (edge).
    ///
    /// - `button`: device-local button index.
    ButtonReleased { button: u16 },

    /// POV hat (D-pad) change.
    ///
    /// Two reporting schemes are supported; producers should pick **one** consistently:
    ///
    /// - **Slot mode (recommended):** `value = -1` for neutral; `0..=7` for
    ///   the 8 cardinal/intercardinal directions in clockwise order:
    ///   `0=N, 1=NE, 2=E, 3=SE, 4=S, 5=SW, 6=W, 7=NW`.
    /// - **Degrees mode:** `value` encodes an angle in degrees (e.g., `0, 45, 90, …`);
    ///   neutral should be `-1`.
    ///
    /// Consumers should treat `-1` as neutral in either mode and handle both schemes.
    HatChanged { hat: u16, value: i16 }, // e.g., -1 (neutral) or 0..7 (N..NW) or degrees
}
