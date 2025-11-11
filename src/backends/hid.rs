//! HID backend for `stickup`.
//!
//! This module exposes a `Device` implementation backed by [`hidapi`] and a simple
//! device discovery helper. It is intended to provide a pragmatic default for
//! game controllers and similar HID devices.
//!
//! - **Feature gate:** enabled by the crate feature `hid`.
//! - **Backend:** [`hidapi`] (cross-platform).
//! - **Policy:** conservative device filtering on Windows (accepts Joystick, Gamepad,
//!   Multi-axis Controller; rejects Keyboard/Mouse and vendor RGB/aux endpoints).
//!
//! # Overview
//! - [`HidInputDevice`]: concrete `Device` backed by `hidapi::HidDevice`.
//! - [`probe_devices`]: enumerate HID devices and wrap them as trait objects.
//! - [`ReportParser`]: pluggable parser interface (payload → [`InputKind`] events).
//! - [`DeviceFingerprint`]: stable identifier built from VID/PID[/Serial].
//!
//! # Polling & coalescing
//! Each call to [`Device::poll`](crate::device::Device::poll) non-blockingly drains up to
//! [`MAX_REPORTS_PER_TICK`] input reports from the device and converts them into a batch
//! of [`InputKind`] events. Buffer growth is amortized and bounded by [`BUF_CAP_LEN`].
//!
//! # Parsing strategy
//! Devices can provide a custom [`ReportParser`]. If none is supplied by
//! [`parser_for`], we fall back to a generic little-endian 16-bit stride parser
//! ([`Le16StrideParser`]) that maps packed `u16` values in `[0..65535]` to `[-1.0..1.0]`
//! axis events. This is a safe baseline for many simple controllers, but proper
//! per-device parsers are encouraged for precision and richer inputs (buttons, hats).
//!
//! # Platform notes
//! - **Windows:** reports are expected to carry a Report ID in byte 0; we split
//!   `(report_id, payload)` accordingly (see [`report_parts_from`]) and apply usage
//!   page / usage filtering in [`accept_device`].
//! - **Non-Windows:** we do not rely on byte-0 report IDs; the entire buffer is
//!   treated as payload by default.
//!
//! # Errors & fallbacks
//! - Device opening failures cause [`HidInputDevice::new`] to return `None`.
//! - Read errors during polling (including WouldBlock and transient I/O) end the
//!   current tick without propagating errors; the next `poll()` will attempt to
//!   read again.
//! - If no parser is present (unexpected), a last-ditch single-byte axis heuristic
//!   produces coarse movement events to avoid returning nothing.
//!
//! # Examples
//! Basic discovery and polling (requires real hardware):
//! ```ignore
//! use hidapi::HidApi;
//! use stickup::backends::hid::probe_devices;
//! use stickup::device::Device;
//!
//! fn main() {
//!     let api = HidApi::new().expect("init hidapi");
//!     let mut devices = probe_devices(&api);
//!     // Poll all devices this frame:
//!     for dev in devices.iter_mut() {
//!         let events = dev.poll();
//!         for e in events {
//!             println!("{:?} -> {:?}", dev.name(), e);
//!         }
//!     }
//! }
//! ```
//!
//! ## API Notes
//! - **Types:** [`HidInputDevice`], [`DeviceFingerprint`], [`ParseCtx`],
//!   [`ChannelDesc`], [`ChannelKind`], [`ReportParser`].
//! - **Functions:** [`probe_devices`], [`accept_device`] (internal), [`parser_for`] (extensible).
//! - **Semantics:** non-blocking reads; per-tick report cap via [`MAX_REPORTS_PER_TICK`] to
//!   avoid starvation; buffer grows lazily up to [`BUF_CAP_LEN`].
//!
//! [`hidapi`]: https://docs.rs/hidapi
#[cfg(target_os = "windows")]
use crate::backends::win_hidp;
use crate::device::Device;
use crate::event::InputKind;
use crate::metadata::DeviceMeta;
use hidapi::{DeviceInfo, HidApi};
#[cfg(target_os = "windows")]
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Max input reports drained from a single device in one `poll()` call.
///
/// This prevents an unusually chatty device from monopolizing a frame and
/// starving other devices.
///
/// Default: `32`.
const MAX_REPORTS_PER_TICK: usize = 32;

/// Initial capacity for the reusable HID input buffer.
///
/// The buffer grows geometrically when a report exactly fills it, up to
/// [`BUF_CAP_LEN`].
const START_BUF_LEN: usize = 64;

/// Hard cap for the reusable HID input buffer capacity.
///
/// Prevents unbounded growth due to pathological report sizes.
const BUF_CAP_LEN: usize = 512;

#[cfg(target_os = "windows")]
static HID_POLL_STATE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new(); // fingerprint -> disabled

/// HID device implementation of the [`Device`] trait.
///
/// Wraps a `hidapi::HidDevice`, maintains a reusable read buffer, and delegates
/// binary payload decoding to a pluggable [`ReportParser`].
pub struct HidInputDevice {
    fingerprint: DeviceFingerprint,
    fingerprint_str: String,
    name: String,
    raw: hidapi::HidDevice,
    buf: Vec<u8>,                                 // reusable read buffer
    buf_cap: usize,                               // upper bound so we don't grow forever
    parser: Option<Box<dyn ReportParser + Send>>, // pluggable report parser
    meta: DeviceMeta,                             // cached metadata snapshot
}

impl HidInputDevice {
    /// Opens `info` as a non-blocking `HidDevice` and constructs a [`HidInputDevice`].
    ///
    /// Returns `None` if the device cannot be opened.
    ///
    /// Caches a [`DeviceFingerprint`] and [`DeviceMeta`] for stable identification.
    ///
    /// On Windows, also records `usage_page` and `usage` from `hidapi::DeviceInfo`
    /// to support filtering and higher-level policies.
    pub fn new(info: &DeviceInfo, api: &HidApi) -> Option<Self> {
        let device = info.open_device(api).ok()?;
        let _ = device.set_blocking_mode(false); // nonblocking = no per-frame stalls

        let fingerprint = DeviceFingerprint::from_info(info);
        let parser = parser_for(info);

        // Cross-platform usage fields
        #[cfg(target_os = "windows")]
        let usage_page = Some(info.usage_page());
        #[cfg(target_os = "windows")]
        let usage = Some(info.usage());
        #[cfg(not(target_os = "windows"))]
        let usage_page: Option<u16> = None;
        #[cfg(not(target_os = "windows"))]
        let usage: Option<u16> = None;

        // Interface number is i32 in hidapi; keep as Option<i32> in our metadata
        let interface_number: Option<i32> = {
            let n: i32 = info.interface_number();
            // Some platforms use -1 for “not applicable”
            if n >= 0 {
                Some(n)
            } else {
                None
            }
        };

        // Path is always present in hidapi
        let path = Some(info.path().to_string_lossy().to_string());

        // Build DeviceMeta once and cache it
        let meta = DeviceMeta {
            bus: Some("usb".into()),
            vid: Some(info.vendor_id()),
            pid: Some(info.product_id()),
            product_string: info.product_string().map(|s| s.to_string()),
            serial_number: info.serial_number().map(|s| s.to_string()),
            usage_page,
            usage,
            interface_number,
            container_id: None,
            path,
        };

        // DEBUG: log device open once so we can correlate which physical device is being read.
        eprintln!(
            "[HID/OPEN] vid=0x{vid:04x} pid=0x{pid:04x} serial={serial} product={product} path={path} usage_page={usage_page:?} usage={usage:?} fingerprint={fp}",
            vid = info.vendor_id(),
            pid = info.product_id(),
            serial = info.serial_number().unwrap_or(""),
            product = info.product_string().unwrap_or(""),
            path = info.path().to_string_lossy(),
            usage_page = { #[cfg(target_os = "windows")] { Some(info.usage_page()) } #[cfg(not(target_os = "windows"))] { None } },
            usage = { #[cfg(target_os = "windows")] { Some(info.usage()) } #[cfg(not(target_os = "windows"))] { None } },
            fp = fingerprint.to_string(),
        );

        Some(Self {
            fingerprint_str: fingerprint.to_string(),
            fingerprint,
            name: info.product_string().unwrap_or("Unknown").to_string(),
            raw: device,
            buf: vec![0u8; START_BUF_LEN],
            buf_cap: BUF_CAP_LEN,
            parser: Some(parser),
            meta,
        })
    }
}

impl Device for HidInputDevice {
    /// Non-blocking read of up to [`MAX_REPORTS_PER_TICK`] HID reports, decoded into [`InputKind`] events.
    ///
    /// - Returns a batch of events observed during this tick (may be empty).
    /// - Transient read errors (including WouldBlock) end the loop for this tick without error.
    /// - The internal buffer grows when a report exactly fills it, up to [`BUF_CAP_LEN`].
    fn poll(&mut self) -> Vec<InputKind> {
        let mut events = Vec::new();
        let mut drained = 0usize;

        loop {
            if drained >= MAX_REPORTS_PER_TICK {
                break;
            }
            match self.raw.read(&mut self.buf) {
                Ok(0) => {
                    // DEBUG: explicit "no data" so we know reads are happening but empty.
                    //eprintln!(
                    //    "[HID/READ] dev={} no data (Ok(0) from read/read_timeout)",
                    //    self.fingerprint_str
                    //);

                    // Fallback probe: some Windows HID devices don't push often via interrupt
                    // endpoint; try an on-demand sample via get_input_report for common IDs.
                    // This is noisy-but-safe and only runs when the interrupt path was empty.
                    let mut polled_any = false;

                    #[cfg(target_os = "windows")]
                    {
                        // If we already learned this device hates poll fallback, bail silently.
                        let disabled = {
                            let map = HID_POLL_STATE
                                .get_or_init(|| Mutex::new(HashMap::new()))
                                .lock()
                                .unwrap();
                            *map.get(&self.fingerprint_str).unwrap_or(&false)
                        };
                        if disabled {
                            break; // nothing to do this tick
                        }
                    }

                    // Most universal attempt: report id = 0 only.
                    for &probe_id in &[0u8] {
                        let mut tmp = vec![0u8; self.buf.len()];
                        tmp[0] = probe_id; // required for get_input_report on Windows
                                           // On Windows prefer INPUT reports; elsewhere keep FEATURE probe.

                        #[cfg(target_os = "windows")]
                        let probe_res = self.raw.get_feature_report(&mut tmp); // may be unsupported on many HIDs
                        #[cfg(not(target_os = "windows"))]
                        let probe_res = self.raw.get_feature_report(&mut tmp); // unchanged off Windows
                        match probe_res {
                            Ok(m) if m > 0 => {
                                polled_any = true;
                                // Parse the polled packet exactly like a normal interrupt read.
                                let (rid_eff, payload) = report_parts_from(&tmp[..m]);
                                let __ev_before = events.len();
                                if let Some(p) = self.parser.as_mut() {
                                    let ctx = ParseCtx {
                                        report_id: rid_eff,
                                        now: Instant::now(),
                                        meta: &self.meta,
                                        fingerprint: &self.fingerprint,
                                    };
                                    p.parse(&ctx, payload, &mut events);
                                } else {
                                    eprintln!(
                                        "[HID/PARSE] dev={} report_id={} parser=None (poll fallback)",
                                        self.fingerprint_str, rid_eff
                                    );
                                    if let Some(&b0) = payload.first() {
                                        let v = (b0 as f32 - 127.5) / 127.5;
                                        events.push(InputKind::AxisMoved { axis: 0, value: v });
                                    }
                                }
                                // DEBUG: count events produced by this polled report.
                                let __new_events = &events[__ev_before..];
                                let __dbg = format!("{:?}", __new_events);
                                let __axis = __dbg.matches("AxisMoved").count();
                                let __btn = __dbg.matches("Button").count();
                                eprintln!(
                                    "[HID/POLL] dev={dev} bytes={n} report_id={rid} axis_moved={axis} buttons={btn}",
                                    dev = self.fingerprint_str,
                                    n = m,
                                    rid = rid_eff,
                                    axis = __axis,
                                    btn = __btn
                                );
                                break; // one successful probe is enough this tick
                            }
                            Ok(_) => {
                                // No bytes from this probe id; try next.
                            }
                            Err(e) => {
                                #[cfg(target_os = "windows")]
                                {
                                    let msg = format!("{:?}", e);
                                    eprintln!(
                                        "[HID/POLL] dev={} get_input_report(id={}) error: {}",
                                        self.fingerprint_str, probe_id, msg
                                    );
                                    // If it's ERROR_INVALID_PARAMETER (0x57), permanently disable poll fallback for this device.
                                    if msg.contains("0x00000057") {
                                        let mut map = HID_POLL_STATE
                                            .get_or_init(|| Mutex::new(HashMap::new()))
                                            .lock()
                                            .unwrap();
                                        if !*map.get(&self.fingerprint_str).unwrap_or(&false) {
                                            map.insert(self.fingerprint_str.clone(), true);
                                            eprintln!(
                                                "[HID/POLL] dev={} disabling poll fallback (ERROR_INVALID_PARAMETER)",
                                                self.fingerprint_str
                                            );
                                        }
                                        break; // stop probing this tick too
                                    }
                                }
                                #[cfg(not(target_os = "windows"))]
                                eprintln!(
                                    "[HID/POLL] dev={} get_feature_report(id={}) error: {:?}",
                                    self.fingerprint_str, probe_id, e
                                );
                            }
                        }
                    }
                    if polled_any {
                        // We produced events via poll; continue loop so caller sees them.
                        break;
                    } else {
                        break; // nothing to do this tick
                    }
                }
                Ok(n) => {
                    drained += 1;
                    let __ev_before = events.len();

                    let (report_id, payload) = report_parts_from(&self.buf[..n]);
                    if let Some(p) = self.parser.as_mut() {
                        let ctx = ParseCtx {
                            report_id,
                            now: Instant::now(),
                            meta: &self.meta,
                            fingerprint: &self.fingerprint,
                        };
                        p.parse(&ctx, payload, &mut events);
                    } else {
                        // DEBUG: parser not initialized; log once per hit.
                        eprintln!(
                            "[HID/PARSE] dev={} report_id={} parser=None (using ultra-fallback)",
                            self.fingerprint_str, report_id
                        );
                        if let Some(&b0) = payload.first() {
                            let v = (b0 as f32 - 127.5) / 127.5;
                            events.push(InputKind::AxisMoved { axis: 0, value: v });
                        }
                    }

                    // Grow buffer if we exactly filled it, up to cap.
                    if n == self.buf.len() && self.buf.len() < self.buf_cap {
                        self.buf.resize((self.buf.len() * 2).min(self.buf_cap), 0);
                    }
                }
                Err(e) => {
                    // DEBUG: surface the underlying hid error instead of silently breaking.
                    eprintln!(
                        "[HID/ERROR] dev={} read failed: {:?}",
                        self.fingerprint_str, e
                    );
                    break; // stop this tick
                } // WouldBlock or device error -> stop this tick
            }
        }
        events
    }

    /// Human-readable product name from HID metadata (falls back to `"Unknown"`).
    fn name(&self) -> &str {
        &self.name
    }

    /// Stable identifier string `VID:PID[:SER]` derived from [`DeviceFingerprint`].
    fn id(&self) -> &str {
        &self.fingerprint_str
    }

    /// Returns a cloned snapshot of cached HID metadata.
    fn metadata(&self) -> DeviceMeta {
        self.meta.clone()
    }
}

/// Discover available HID devices and wrap them as trait objects.
///
/// Applies platform-specific filtering (see [`accept_device`]) and constructs a
/// [`HidInputDevice`] for each accepted entry that can be opened.
///
/// The returned `Vec` contains boxed `dyn Device` to allow heterogeneous backends.
///
/// # Example
/// ```ignore
/// let api = hidapi::HidApi::new().unwrap();
/// let devices = stickup::backends::hid::probe_devices(&api);
/// println!("found {} HID devices", devices.len());
/// ```
pub fn probe_devices(api: &HidApi) -> Vec<Box<dyn Device>> {
    let mut found = Vec::new();
    for info in api.device_list() {
        if !accept_device(info) {
            continue;
        }
        if let Some(dev) = HidInputDevice::new(info, api) {
            found.push(Box::new(dev) as Box<dyn Device>);
        }
    }
    found
}

/// Decide whether we should attach to this HID interface.
///
/// On **Windows**, we filter strictly by Usage Page and Usage to avoid keyboards,
/// mice, and vendor-specific RGB/aux endpoints:
/// - Usage Page must be **Generic Desktop (0x01)**.
/// - Accept Usages: **Joystick (0x04)**, **Gamepad (0x05)**, **Multi-axis Controller (0x08)**.
/// - Reject Usages: **Mouse (0x02)**, **Keyboard (0x06)**.
///
/// On other platforms this function currently returns `true` for all devices.
///
/// This policy favors game controller inputs and avoids capturing core system
/// devices that users expect to remain untouched.
fn accept_device(info: &DeviceInfo) -> bool {
    // Strict Windows filter by usage page/usage to avoid keyboards/mice/vendor RGB endpoints.
    #[cfg(target_os = "windows")]
    {
        let up = info.usage_page();
        let u = info.usage();

        // Generic Desktop only
        if up != 0x01 {
            return false;
        }

        // Accept: Joystick (0x04), Gamepad (0x05), Multi-axis Controller (0x08)
        let is_game_controller = matches!(u, 0x04 | 0x05 | 0x08);
        // Reject: Mouse (0x02), Keyboard (0x06)
        let is_mouse_or_kbd = matches!(u, 0x02 | 0x06);

        if !is_game_controller || is_mouse_or_kbd {
            return false;
        }
    }

    true
}

/// Unique identifier for a physical device.
///
/// Combines Vendor ID (VID), Product ID (PID), and optional serial number.
/// Use [`to_string`](DeviceFingerprint::to_string) for a stable textual ID suitable
/// for persistence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceFingerprint {
    /// USB Vendor ID.
    pub vendor_id: u16,
    /// USB Product ID.
    pub product_id: u16,
    /// Optional device serial number (if provided by firmware/OS).
    pub serial_number: Option<String>,
    /// Optional HID device path (used as a uniqueness fallback if no serial).
    pub path: Option<String>,
}

impl DeviceFingerprint {
    /// Build a fingerprint from `hidapi::DeviceInfo`.
    pub fn from_info(info: &hidapi::DeviceInfo) -> Self {
        Self {
            vendor_id: info.vendor_id(),
            product_id: info.product_id(),
            serial_number: info.serial_number().map(|s| s.to_owned()),
            path: Some(info.path().to_string_lossy().to_string()),
        }
    }

    /// Format as a persistent ID string: `"{VID:04x}:{PID:04x}[:{SER}]"`.
    pub fn to_string(&self) -> String {
        if let Some(serial) = &self.serial_number {
            return format!("{:04x}:{:04x}:{}", self.vendor_id, self.product_id, serial);
        }
        if let Some(path) = &self.path {
            // Keep it short/stable: use the last path segment.
            // Hold the normalized string so the &str we take lives long enough.
            let norm = path.replace('\\', "/");
            let seg: &str = norm.rsplit('/').next().unwrap_or(norm.as_str());
            return format!("{:04x}:{:04x}@{}", self.vendor_id, self.product_id, seg);
        }
        format!("{:04x}:{:04x}", self.vendor_id, self.product_id)
    }
}

/* -----------------------------
   Report parsing infrastructure
------------------------------*/

/// Split `(report_id, payload)` from a raw HID report.
///
/// On **Windows**, byte 0 is always the Report ID; payload starts at byte 1.
/// On other platforms, we do not assume a leading Report ID and treat the entire
/// buffer as payload while returning `report_id = 0`.
#[inline]
fn report_parts_from(data: &[u8]) -> (u8, &[u8]) {
    #[cfg(target_os = "windows")]
    {
        if !data.is_empty() {
            let report_id = data[0];
            let payload = if data.len() > 1 { &data[1..] } else { &[] };
            (report_id, payload)
        } else {
            (0, &[])
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Many stacks include report_id=0 at byte 0, but not guaranteed.
        // Treat as unknown (0) for now; payload is the whole buffer.
        (0, data)
    }
}

/// Parsing context for a single HID report.
///
/// Provided to [`ReportParser::parse`] so decoders can consult time, metadata,
/// and the stable device fingerprint if needed.
pub struct ParseCtx<'a> {
    /// Report ID (0 if unknown).
    pub report_id: u8,
    /// Timestamp captured at the start of decode for this report.
    pub now: Instant,
    /// Cached metadata snapshot for the device.
    pub meta: &'a DeviceMeta,
    /// Stable fingerprint for the device.
    pub fingerprint: &'a DeviceFingerprint,
}

/// Minimal interface for decoding a device's report payload into input events.
///
/// Parsers receive a [`ParseCtx`], the raw `payload` bytes (report body), and a
/// mutable `out` sink to which they should push zero or more [`InputKind`] events.
pub trait ReportParser: Send {
    /// Optionally declare channels (axes/buttons/hats) known to this device.
    ///
    /// Returning channel descriptors helps downstream UIs pre-label inputs or
    /// allocate storage. The default implementation returns an empty list.
    fn describe(&self) -> Vec<ChannelDesc> {
        Vec::new()
    }

    /// Decode one report payload into events.
    ///
    /// Implementations should avoid emitting noisy events by coalescing small
    /// deltas or duplicate values as appropriate for the device.
    fn parse(&mut self, ctx: &ParseCtx, payload: &[u8], out: &mut Vec<InputKind>);
}

/// Categories of logical input channels a parser may expose.
#[derive(Clone, Debug)]
pub enum ChannelKind {
    /// Continuous axis (e.g., X/Y, throttle, rudder).
    Axis,
    /// Binary or latching control.
    Button,
    /// Directional hat switch (POV).
    Hat,
}

/// Descriptor for a single logical channel produced by a device/parser.
///
/// Useful for UI labeling and input mapping UIs.
#[derive(Clone, Debug)]
pub struct ChannelDesc {
    /// Channel category.
    pub kind: ChannelKind,
    /// Stable per-device index for this channel.
    pub idx: u16,
    /// Optional human-readable name.
    pub name: Option<String>,
    /// Logical minimum in the device's native units.
    pub logical_min: i32,
    /// Logical maximum in the device's native units.
    pub logical_max: i32,
    /// Optional HID Usage Page associated with this channel.
    pub usage_page: Option<u16>,
    /// Optional HID Usage associated with this channel.
    pub usage: Option<u16>,
}

/// Generic fallback parser: little-endian `u16` stride mapped to `[-1.0, 1.0]` axes.
///
/// Treats each consecutive 2-byte chunk as an axis sample. Emits [`InputKind::AxisMoved`]
/// events when the value changes by more than a small epsilon to reduce noise.
struct Le16StrideParser {
    last: Vec<f32>, // coalescing per-axis
    eps: f32,
}

impl Le16StrideParser {
    /// Construct a default stride parser with a small coalescing epsilon (`1e-3`).
    fn new() -> Self {
        Self {
            last: Vec::new(),
            eps: 0.001,
        }
    }
}

impl ReportParser for Le16StrideParser {
    fn parse(&mut self, _ctx: &ParseCtx, payload: &[u8], out: &mut Vec<InputKind>) {
        let mut i = 0usize;
        // Safety: stop before an odd trailing byte.
        let mut axis_idx = 0usize;
        while i + 1 < payload.len() {
            let raw = u16::from_le_bytes([payload[i], payload[i + 1]]) as f32;
            // Assume logical 0..65535 for the generic fallback:
            let v = raw / 65535.0 * 2.0 - 1.0; // normalize to [-1,1]
            if axis_idx >= self.last.len() {
                self.last.resize(axis_idx + 1, 0.0);
            }
            if (v - self.last[axis_idx]).abs() > self.eps {
                self.last[axis_idx] = v;
                out.push(InputKind::AxisMoved {
                    axis: axis_idx as u16,
                    value: v,
                });
            }
            axis_idx += 1;
            i += 2;
        }
    }
}

/// Choose a parser for a device (by VID/PID, usage, etc.).
///
/// Extend this function with per-device specializations as needed. The default
/// returns the generic [`Le16StrideParser`].
fn parser_for(info: &DeviceInfo) -> Box<dyn ReportParser + Send> {
    // Example specialization:
    // match (_info.vendor_id(), _info.product_id()) {
    //     (0x046d, 0xc216) => Box::new(SomeOtherParser),
    //     _ => Box::new(Le16StrideParser),
    // }
    #[cfg(target_os = "windows")]
    if let Some(p) = win_hidp::WinHidpParser::new(info) {
        return Box::new(p);
    }
    Box::new(Le16StrideParser::new())
}
