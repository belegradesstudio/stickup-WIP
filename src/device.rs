//! Device traits and parsing contracts.
//!
//! This module defines the core traits used by StickUp backends.
//!
//! ## Design goals
//! - **Stable device identity:** device IDs should remain stable across reconnects/USB-port changes
//!   where possible (fingerprints/serials when available).
//! - **Deterministic channel layout:** [`Device::describe`] should return channels in a stable order
//!   and use the same indices that appear in [`InputKind`] events.
//! - **Polling yields deltas:** [`Device::poll`] returns input changes since the last poll. The
//!   [`Manager`](crate::manager::Manager) maintains last-known state by applying these deltas.
//!
//! StickUp reads devices. It does not create virtual devices.

use crate::event::{ChannelDesc, InputKind};
use crate::DeviceMeta;
use std::time::Instant;

/// A device identity fingerprint suitable for stable binding / persistence.
///
/// Backends should populate as much as possible. Prefer real serial numbers when available.
/// When serial is missing, a (normalized) device path segment can still provide stability
/// across a single machine, but may change with driver/port changes.
///
/// This is used for:
/// - stable device IDs (`Device::id`)
/// - binding persistence (device re-identification)
/// - future calibration storage keys
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceFingerprint {
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: Option<String>,
    pub path: Option<String>,
}

impl DeviceFingerprint {
    /// Convert fingerprint into a stable-ish string key.
    ///
    /// Priority:
    /// 1) `vid:pid:serial` when serial exists
    /// 2) `vid:pid@<last_path_segment>` when only a path exists
    /// 3) `vid:pid` as a last resort (not unique if multiple identical devices exist)
    pub fn to_string(&self) -> String {
        if let Some(serial) = &self.serial_number {
            return format!("{:04x}:{:04x}:{}", self.vendor_id, self.product_id, serial);
        }
        if let Some(path) = &self.path {
            let norm = path.replace('\\', "/");
            let seg: &str = norm.rsplit('/').next().unwrap_or(norm.as_str());
            return format!("{:04x}:{:04x}@{}", self.vendor_id, self.product_id, seg);
        }
        format!("{:04x}:{:04x}", self.vendor_id, self.product_id)
    }
}

/// Context passed to report parsers during decode.
///
/// - `report_id` is the report ID byte (or 0 for single-report devices)
/// - `now` is a monotonic timestamp captured by the caller
/// - `meta` and `fingerprint` provide device identity and descriptors that parsers can use
///   for quirks, naming, or future calibration hooks.
pub struct ParseCtx<'a> {
    pub report_id: u8,
    pub now: Instant,
    pub meta: &'a DeviceMeta,
    pub fingerprint: &'a DeviceFingerprint,
}

/// Parser for raw HID input reports.
///
/// Backends use this trait to decode OS-provided report bytes into StickUp events.
///
/// Contract:
/// - Events emitted from [`ReportParser::parse`] must use axis/button/hat indices that
///   match the channel layout returned by [`ReportParser::describe`].
/// - `describe()` should be deterministic (stable ordering) so bindings/UI can rely on it.
/// - Parsers may be stateful (e.g. for edge detection, hat decoding, quirks), so `&mut self`.
pub trait ReportParser: Send {
    /// Exact input report size (including the report ID byte), if known.
    ///
    /// If `Some(n)`, the backend may allocate a buffer of exactly `n` for reads.
    /// If `None`, the backend may use a conservative maximum size.
    fn input_report_len(&self) -> Option<usize> {
        None
    }

    /// Describe channels in a stable, deterministic order.
    ///
    /// This is used for UI display and for building stable names/labels. The indices
    /// must correspond to the indices emitted in [`InputKind`] events.
    fn describe(&self) -> Vec<ChannelDesc> {
        Vec::new()
    }

    /// Parse a report **payload** (bytes after the report ID) into events.
    ///
    /// Implementations should push deltas into `out` (axis moves, button edges, hat changes).
    /// The caller controls allocation by reusing `out`.
    fn parse(&mut self, ctx: &ParseCtx, payload: &[u8], out: &mut Vec<InputKind>);

    /// Does the raw input buffer include a leading report ID byte?
    ///
    /// - `true`  => reports come in as `[report_id][payload...]`
    /// - `false` => reports come in as `[payload...]` (single-report device, ReportID == 0)
    ///
    /// Default is `true`.
    fn expects_report_id_prefix(&self) -> bool {
        true
    }
}

/// A concrete input device managed by StickUp.
///
/// Backends implement this trait to expose devices to [`Manager`](crate::manager::Manager).
///
/// Contract:
/// - [`Device::id`] should be stable across polls and ideally across reconnects.
/// - [`Device::poll`] should return **deltas since the last poll** (not a full state dump).
/// - [`Device::describe`] should describe the device’s channel layout and match emitted indices.
pub trait Device {
    /// Poll the device and return any input changes since the last poll.
    fn poll(&mut self) -> Vec<InputKind>;

    /// Human-friendly device name for UI display.
    fn name(&self) -> &str;

    /// Stable device identifier used as the key in manager maps and bindings.
    ///
    /// Backends should prefer fingerprint-based IDs (vid/pid + serial when possible).
    fn id(&self) -> &str;

    /// Device metadata (bus, vid/pid, path, etc.) for UI/diagnostics.
    fn metadata(&self) -> DeviceMeta;

    /// Describe available channels (axes/buttons/hats).
    ///
    /// Indices must match indices used in [`InputKind`] emitted by [`poll`](Device::poll).
    fn describe(&self) -> Vec<ChannelDesc>;
}
