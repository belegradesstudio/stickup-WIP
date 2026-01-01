//! Events and channel descriptions.
//!
//! StickUp represents input changes as small, device-agnostic deltas ([`InputKind`]) and
//! optionally timestamps them ([`InputEvent`]).
//!
//! ## Value conventions
//! - **HID / XInput axes:** by convention are normalized to `[-1.0, 1.0]`.
//! - **Buttons:** boolean state expressed as press/release edges.
//! - **Hats (POV/D-pad):** `-1` = neutral, `0..7` = 8-way directions (Up = 0, clockwise).
//!
//! ### Important: units may vary by source
//! `InputKind::AxisMoved` is used for multiple input sources:
//! - For typical controller axes it is normalized (`[-1, 1]`).
//! - For some injected sources (e.g. Windows Raw Input mouse deltas / wheel ticks),
//!   the value may be **raw counts** or **tick units** rather than normalized.
//!
//! If you need strict normalization, apply it at the application layer (or introduce a
//! dedicated mouse event type in a future release).
//!
//! This crate currently follows the “raw truth” approach for injected mouse input:
//! it preserves the units reported by the OS. A later version may add dedicated mouse
//! event variants for clearer semantics.

/// Per-device input change (delta).
///
/// The `axis`/`button`/`hat` indices are device-local channel indices as described by [`ChannelDesc`].
#[derive(Clone, Debug)]
pub enum InputKind {
    /// A continuous channel changed.
    ///
    /// Convention: `value` is normalized to `[-1.0, 1.0]` for typical controller axes.
    /// Some injected sources may use raw units (see module docs).
    AxisMoved { axis: u16, value: f32 },

    /// A button transitioned to pressed.
    ButtonPressed { button: u16 },

    /// A button transitioned to released.
    ButtonReleased { button: u16 },

    /// A hat (POV/D-pad) changed.
    ///
    /// `value`: `-1` = neutral, `0..7` = directions (Up = 0, clockwise).
    HatChanged { hat: u16, value: i16 },
}

/// Timestamped input event captured by the Manager.
///
/// This is a lightweight wrapper over [`InputKind`] with a monotonic timestamp.
#[derive(Clone, Debug)]
pub struct InputEvent {
    /// Capture time (monotonic). Suitable for ordering / delta timing within a run.
    pub at: std::time::Instant,
    /// The actual input change.
    pub kind: InputKind,
}

/// Category of an input channel on a device.
#[derive(Clone, Debug, PartialEq)]
pub enum ChannelKind {
    Axis,
    Button,
    Hat,
}

/// Describes a channel exposed by a device.
///
/// Backends typically populate this from device descriptors (HIDP, XInput layout, etc.)
/// so UIs and binding systems can present stable channel names and ranges.
#[derive(Clone, Debug)]
pub struct ChannelDesc {
    /// Channel category.
    pub kind: ChannelKind,
    /// Device-local channel index (matches indices used in [`InputKind`]).
    pub idx: u16,
    /// Optional human-friendly name (e.g. `"X"`, `"Y"`, `"Trigger"`, `"hat0"`).
    pub name: Option<String>,
    /// Backend-provided logical min (descriptor range). Not necessarily normalized.
    pub logical_min: i32,
    /// Backend-provided logical max (descriptor range). Not necessarily normalized.
    pub logical_max: i32,
    /// Optional HID usage page (when available).
    pub usage_page: Option<u16>,
    /// Optional HID usage (when available).
    pub usage: Option<u16>,
}
