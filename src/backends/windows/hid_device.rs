#![cfg(target_os = "windows")]

//! Windows HID device wrapper.
//!
//! [`HidInputDevice`] wraps a `hidapi::HidDevice` and a [`ReportParser`] implementation.
//! It is responsible for:
//! - opening the HID handle in non-blocking mode
//! - draining a bounded number of reports per poll
//! - splitting `[report_id][payload...]` vs `[payload...]` depending on parser expectations
//! - translating raw report bytes into [`InputKind`] deltas via the parser
//!
//! This module does **not**:
//! - maintain an accumulated state map (that is `Manager`’s job)
//! - apply deadzones/curves/smoothing (that is binding/UI policy)
//! - create any kind of virtual device output

use crate::device::{Device, DeviceFingerprint, ParseCtx, ReportParser};
use crate::event::{ChannelDesc, InputKind};
use crate::metadata::DeviceMeta;
use hidapi::{DeviceInfo, HidApi};
use std::time::Instant;

/// Safety valve: maximum number of HID reports drained per `poll()` call.
///
/// Prevents a single device from starving the rest of the system if it is
/// producing data faster than the host is polling.
const MAX_REPORTS_PER_TICK: usize = 32;

/// Concrete HID-backed device implementing [`Device`](crate::device::Device).
///
/// The device’s stable ID is derived from its [`DeviceFingerprint`]. See `device.rs`
/// for identity/persistence notes.
pub struct HidInputDevice {
    fingerprint: DeviceFingerprint,
    fingerprint_str: String,
    name: String,
    raw: hidapi::HidDevice,
    buf: Vec<u8>,                         // exactly input_report_len
    parser: Box<dyn ReportParser + Send>, // non-optional
    meta: DeviceMeta,
}

impl HidInputDevice {
    /// Attempt to open and wrap a HID device entry.
    ///
    /// Returns `None` if the OS handle cannot be opened. The parser is required and non-optional.
    pub fn new(
        info: &DeviceInfo,
        api: &HidApi,
        parser: impl ReportParser + Send + 'static,
        fingerprint: DeviceFingerprint,
        meta: DeviceMeta,
    ) -> Option<Self> {
        let device = info.open_device(api).ok()?;
        // StickUp polls devices in a host-controlled loop, so we use non-blocking reads.
        // If set_blocking_mode fails, we continue anyway; `hidapi` will still error or
        // return data depending on backend behavior.
        let _ = device.set_blocking_mode(false);

        let boxed: Box<dyn ReportParser + Send> = Box::new(parser);
        // Allocate to the exact size (including ID byte) if known; otherwise a safe default.
        let buf_len = boxed.input_report_len().unwrap_or(64);
        let buf = vec![0u8; buf_len];

        let name = info.product_string().unwrap_or("Unknown").to_string();

        // NOTE: this is intentionally verbose for development and device bring-up.
        #[cfg(all(feature = "debug-log", debug_assertions))]
        eprintln!(
            "[HID/OPEN] vid=0x{vid:04x} pid=0x{pid:04x} serial={serial} product={product} path={path} usage_page={up:?} usage={u:?} fingerprint={fp}",
            vid = info.vendor_id(),
            pid = info.product_id(),
            serial = info.serial_number().unwrap_or(""),
            product = info.product_string().unwrap_or(""),
            path = info.path().to_string_lossy(),
            up = Some(info.usage_page()),
            u  = Some(info.usage()),
            fp = fingerprint.to_string(),
        );

        Some(Self {
            fingerprint_str: fingerprint.to_string(),
            fingerprint,
            name,
            raw: device,
            buf,
            parser: boxed,
            meta,
        })
    }

    /// Windows-style split: treat the first byte as a report ID.
    ///
    /// Many HID stacks deliver input reports as `[report_id][payload...]` even when the
    /// device only uses a single report. Some devices/parsers instead want the entire
    /// read buffer as payload. That behavior is controlled by
    /// [`ReportParser::expects_report_id_prefix`].
    #[inline]
    fn split_report_windows(data: &[u8]) -> (u8, &[u8]) {
        if !data.is_empty() {
            let report_id = data[0];
            let payload = if data.len() > 1 { &data[1..] } else { &[] };
            (report_id, payload)
        } else {
            (0, &[])
        }
    }
}

impl Device for HidInputDevice {
    /// Drain up to [`MAX_REPORTS_PER_TICK`] reports and return the resulting input deltas.
    ///
    /// This method does not timestamp events itself beyond the [`ParseCtx::now`] field passed
    /// to parsers; higher-level timing wrappers live in `Manager`.
    fn poll(&mut self) -> Vec<InputKind> {
        let mut events = Vec::new();
        let mut drained = 0;

        loop {
            if drained >= MAX_REPORTS_PER_TICK {
                break;
            }

            match self.raw.read(&mut self.buf) {
                Ok(0) => break, // no data this tick (non-blocking)
                Ok(n) => {
                    drained += 1;

                    let slice = &self.buf[..n];

                    // --- DEBUG: log every raw read ---
                    // eprintln!(
                    //     "[HID/READ] dev={} n={} bytes: {:02x?}",
                    //     self.fingerprint_str, n, slice,
                    // );

                    // If the parser expects an ID prefix, treat the first byte as ReportID.
                    // Otherwise, treat the entire slice as payload and use ReportID = 0.
                    let (report_id, payload) = if self.parser.expects_report_id_prefix() {
                        Self::split_report_windows(slice)
                    } else {
                        (0, slice)
                    };

                    let ctx = ParseCtx {
                        report_id,
                        now: Instant::now(),
                        meta: &self.meta,
                        fingerprint: &self.fingerprint,
                    };
                    self.parser.parse(&ctx, payload, &mut events);
                }
                Err(e) => {
                    eprintln!(
                        "[HID/ERROR] dev={} read failed: {:?}",
                        self.fingerprint_str, e
                    );
                    break;
                }
            }
        }
        events
    }

    fn name(&self) -> &str {
        &self.name
    }
    fn id(&self) -> &str {
        &self.fingerprint_str
    }
    fn metadata(&self) -> DeviceMeta {
        self.meta.clone()
    }
    fn describe(&self) -> Vec<ChannelDesc> {
        self.parser.describe()
    }
}
