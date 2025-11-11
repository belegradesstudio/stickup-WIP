#![cfg(windows)]
//! Windows HIDP descriptor-driven parser.
//!
//! This module builds a per-device descriptor map using the Windows HID Parser
//! (HIDP) APIs and decodes input reports accordingly. It handles:
//! - Buttons (bitfields) via `HidP_GetUsages` → `ButtonPressed/Released` edges
//! - Axes (any bit widths, signed/unsigned) via `HidP_GetUsageValue`
//! - Hat Switch (usage 0x39): standardizes to **slot** values (-1/0..7) by default
//!
//! It prefers correctness for HOTAS/HOSAS/pedals without per-brand code.
//!
//! ## Hat policy
//! Devices that report **degrees** are converted to **slots** using 45° sectors:
//! `slot = floor((deg + 22.5) / 45) mod 8`. Neutral is reported as `-1`.
//!
//! ## Notes
//! - We open an OS handle from the HID path and keep it alive alongside the
//!   `PreparsedData` handle for the parser lifetime.
//! - If HIDP calls fail during construction, fall back to your generic parser.
//!
//! ## Dependencies
//! Requires `windows-sys` with HID and FS features (see Cargo.toml).
//!
//! ## Platform & feature gates
//! - Windows only (`#![cfg(windows)]`).
//! - Consumes HID report **descriptors** at init and raw **reports** at `parse()`.
//!
//! ## Example (conceptual)
//! ```ignore
//! use stickup::backends::win_hidp::WinHidpParser;
//! use stickup::backends::hid::{ReportParser, ParseCtx};
//!
//! let info: hidapi::DeviceInfo = /* from HidApi::device_list() */;
//! if let Some(mut p) = WinHidpParser::new(&info) {
//!     // ctx.report_id should be the runtime report ID for `payload`.
//!     let ctx = ParseCtx { report_id: 1, now: std::time::Instant::now(), meta: /* ... */, fingerprint: /* ... */ };
//!     let mut out = Vec::new();
//!     let payload: &[u8] = /* report body excluding ID byte */;
//!     p.parse(&ctx, payload, &mut out);
//! }
//! ```

use core::mem::MaybeUninit;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use hidapi::DeviceInfo;

use crate::event::InputKind;

use super::hid::{ChannelDesc, ChannelKind, ParseCtx, ReportParser};

use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, NTSTATUS,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

const STATUS_SUCCESS: NTSTATUS = HIDP_STATUS_SUCCESS; // alias for clarity

/// One normalized button field (expanded per usage).
#[derive(Clone, Debug)]
struct ButtonField {
    report_id: u8,
    usage_page: u16,
    // The concrete usage codes this cap covers (expanded from range where needed).
    usages: Vec<u16>,
}

/// One normalized value (axis/hat) field.
#[derive(Clone, Debug)]
struct ValueField {
    report_id: u8,
    usage_page: u16,
    usage: u16,
    logical_min: i32,
    logical_max: i32,
    is_hat: bool,            // usage == 0x39 on Generic Desktop
    hat_is_degrees: bool,    // true if descriptor suggests degrees rather than 0..7
    axis_index: Option<u16>, // assigned index for AxisMoved
    hat_index: Option<u16>,  // assigned index for HatChanged
}

/// Descriptor-driven HIDP parser (Windows).
pub struct WinHidpParser {
    handle: HANDLE,
    ppd: PHIDP_PREPARSED_DATA, // opaque handle (isize)
    input_report_max_len: u16,

    // Flattened/normalized fields
    buttons: Vec<ButtonField>,
    values: Vec<ValueField>,

    // Stable index maps (usage → button/axis index)
    button_index_by_usage: HashMap<(u8, u16, u16), u16>, // (report_id, usage_page, usage) → idx
    axis_fields_by_index: Vec<usize>,                    // axis_index → values[] idx
    hat_fields_by_index: Vec<usize>,                     // hat_index → values[] idx

    // Last-frame state for edge/coalesce
    last_pressed_buttons: HashSet<u16>, // button indices currently pressed
    last_axis_value: HashMap<u16, f32>, // axis_index → last value
    last_hat_value: HashMap<u16, i16>,  // hat_index → last slot value
    axis_epsilon: f32,
}

impl Drop for WinHidpParser {
    fn drop(&mut self) {
        unsafe {
            if self.ppd != 0 {
                HidD_FreePreparsedData(self.ppd);
                self.ppd = 0;
            }
            if self.handle != 0 {
                CloseHandle(self.handle);
                self.handle = 0;
            }
        }
    }
}

impl WinHidpParser {
    /// Construct from a `hidapi::DeviceInfo`.
    ///
    /// Returns `None` if the device cannot be opened, its preparsed data cannot
    /// be obtained, or global caps cannot be read. Callers should fall back to a
    /// generic parser in that case.
    ///
    /// ### Behavior
    /// - Opens the OS device handle using the HID path from `DeviceInfo`.
    /// - Calls `HidD_GetPreparsedData` and `HidP_GetCaps` to cache descriptor info.
    /// - Enumerates button/value caps and assigns stable indices.
    pub fn new(info: &DeviceInfo) -> Option<Self> {
        let path = info.path().to_string_lossy().to_string();
        let handle = open_device_handle(&path).ok()?;

        // Get Preparsed Data
        let mut ppd: PHIDP_PREPARSED_DATA = 0;
        let ok = unsafe { HidD_GetPreparsedData(handle, &mut ppd) };
        if ok == 0 || ppd == 0 {
            unsafe { CloseHandle(handle) };
            return None;
        }

        // Caps (global)
        let mut caps = MaybeUninit::<HIDP_CAPS>::uninit();
        let status = unsafe { HidP_GetCaps(ppd, caps.as_mut_ptr()) };
        if status != STATUS_SUCCESS {
            unsafe {
                HidD_FreePreparsedData(ppd);
                CloseHandle(handle);
            }
            return None;
        }
        let caps = unsafe { caps.assume_init() };

        // Enumerate button caps (input report)
        let btn_caps = enumerate_button_caps(ppd, HidP_Input)?;
        // Enumerate value caps (input report)
        let val_caps = enumerate_value_caps(ppd, HidP_Input)?;

        // Normalize caps → fields
        let buttons = normalize_buttons(&btn_caps);
        let mut values = normalize_values(&val_caps);

        // Assign stable indices for axes/hats/buttons
        let mut axis_fields_by_index = Vec::new();
        let mut hat_fields_by_index = Vec::new();
        let mut button_index_by_usage: HashMap<(u8, u16, u16), u16> = HashMap::new();

        // Axis indices
        {
            let mut next_axis: u16 = 0;
            for (i, vf) in values.iter_mut().enumerate() {
                if vf.is_hat {
                    continue;
                }
                vf.axis_index = Some(next_axis);
                axis_fields_by_index.push(i);
                next_axis += 1;
            }
        }
        // Hat indices
        {
            let mut next_hat: u16 = 0;
            for (i, vf) in values.iter_mut().enumerate() {
                if vf.is_hat {
                    vf.hat_index = Some(next_hat);
                    hat_fields_by_index.push(i);
                    next_hat += 1;
                }
            }
        }
        // Button indices are assigned in a stable, deterministic order
        {
            let mut next_btn: u16 = 0;
            for bf in &buttons {
                for &u in &bf.usages {
                    button_index_by_usage.insert((bf.report_id, bf.usage_page, u), next_btn);
                    next_btn += 1;
                }
            }
        }

        Some(Self {
            handle,
            ppd,
            input_report_max_len: caps.InputReportByteLength,
            buttons,
            values,
            button_index_by_usage,
            axis_fields_by_index,
            hat_fields_by_index,
            last_pressed_buttons: HashSet::new(),
            last_axis_value: HashMap::new(),
            last_hat_value: HashMap::new(),
            axis_epsilon: 0.001,
        })
    }
}

impl ReportParser for WinHidpParser {
    /// Return a descriptor list for axes, hats, and buttons derived from HID caps.
    fn describe(&self) -> Vec<ChannelDesc> {
        let mut out = Vec::new();

        // Axes
        for &i in &self.axis_fields_by_index {
            let v = &self.values[i];
            out.push(ChannelDesc {
                kind: ChannelKind::Axis,
                idx: v.axis_index.unwrap_or(0),
                name: usage_name(v.usage_page, v.usage),
                logical_min: v.logical_min,
                logical_max: v.logical_max,
                usage_page: Some(v.usage_page),
                usage: Some(v.usage),
            });
        }
        // Hats
        for &i in &self.hat_fields_by_index {
            let v = &self.values[i];
            out.push(ChannelDesc {
                kind: ChannelKind::Hat,
                idx: v.hat_index.unwrap_or(0),
                name: Some("Hat".to_string()),
                logical_min: 0,
                logical_max: 7,
                usage_page: Some(v.usage_page),
                usage: Some(v.usage),
            });
        }
        // Buttons
        for (key, idx) in &self.button_index_by_usage {
            let (_rid, up, u) = *key;
            out.push(ChannelDesc {
                kind: ChannelKind::Button,
                idx: *idx,
                name: usage_name(up, u),
                logical_min: 0,
                logical_max: 1,
                usage_page: Some(up),
                usage: Some(u),
            });
        }

        out
    }

    /// Decode one input report payload into `InputKind` events using HIDP.
    ///
    /// ### Inputs
    /// - `ctx.report_id`: the runtime Report ID for `payload`.
    /// - `payload`: report body bytes **excluding** the Report ID byte.
    ///
    /// ### Behavior
    /// - Builds a full report buffer `[report_id][payload .. padded ..]` sized to
    ///   `input_report_max_len` and passes it to HIDP.
    /// - Emits edge events for buttons and coalesced deltas for axes.
    /// - Normalizes hats to **slot** values `-1 | 0..7`.
    fn parse(&mut self, ctx: &ParseCtx, payload: &[u8], out: &mut Vec<InputKind>) {
        // HIDP expects the full report including the Report ID byte.
        // Use the device's advertised max input report length so we pass a complete buffer.
        let max = self.input_report_max_len as usize;
        let mut report = vec![0u8; max.max(1)];
        report[0] = ctx.report_id;
        let copy_len = payload.len().min(report.len().saturating_sub(1));
        report[1..1 + copy_len].copy_from_slice(&payload[..copy_len]);
        // HIDP APIs want the FULL input report buffer size (InputReportByteLength).
        // We zero-pad the remainder; pass the full length to HIDP.
        let report_len_full = report.len() as u32;

        // BUTTONS: collect all currently-pressed button indices, then diff vs last
        let mut pressed_now: BTreeSet<u16> = BTreeSet::new();

        for bf in &self.buttons {
            // Skip caps not matching this report ID (unless both are zero)
            if bf.report_id != 0 && bf.report_id != ctx.report_id {
                continue;
            }
            // We need to query the pressed usages for this (report_id, usage_page)
            let mut usage_buf = [0u16; 128];
            let mut usage_len: u32 = usage_buf.len() as u32;

            let status = unsafe {
                HidP_GetUsages(
                    HidP_Input,
                    bf.usage_page,
                    0, // LinkCollection = 0 (typical)
                    usage_buf.as_mut_ptr(),
                    &mut usage_len,
                    self.ppd,
                    report.as_mut_ptr(),
                    report_len_full,
                )
            };

            if status != STATUS_SUCCESS {
                eprintln!(
                    "[HIDP] GetUsages failed: status=0x{:08x} rid={} up=0x{:02x}",
                    status as u32, ctx.report_id, bf.usage_page
                );
                continue;
            }

            // For each pressed usage, map to our stable button index
            for i in 0..(usage_len as usize) {
                let usage = usage_buf[i];
                if let Some(&btn_idx) =
                    self.button_index_by_usage
                        .get(&(ctx.report_id, bf.usage_page, usage))
                {
                    pressed_now.insert(btn_idx);
                } else if let Some(&btn_idx) =
                    // some stacks have report_id=0 in caps even when runtime ID != 0
                    self.button_index_by_usage.get(&(0, bf.usage_page, usage))
                {
                    pressed_now.insert(btn_idx);
                }
            }
        }

        // Emit edges (presses/releases)
        for &idx in pressed_now.iter() {
            if !self.last_pressed_buttons.contains(&idx) {
                out.push(InputKind::ButtonPressed { button: idx });
            }
        }
        for &idx in self.last_pressed_buttons.iter() {
            if !pressed_now.contains(&idx) {
                out.push(InputKind::ButtonReleased { button: idx });
            }
        }
        self.last_pressed_buttons = pressed_now.into_iter().collect();

        // VALUES: axes and hats
        for vf in &self.values {
            // report ID match
            if vf.report_id != 0 && vf.report_id != ctx.report_id {
                continue;
            }

            // Query value
            let mut value: u32 = 0;
            let status = unsafe {
                HidP_GetUsageValue(
                    HidP_Input,
                    vf.usage_page,
                    0, // LinkCollection
                    vf.usage as u16,
                    &mut value,
                    self.ppd,
                    report.as_mut_ptr(),
                    report_len_full,
                )
            };
            if status != STATUS_SUCCESS {
                eprintln!(
                    "[HIDP] GetUsageValue failed: status=0x{:08x} rid={} up=0x{:02x} u=0x{:02x}",
                    status as u32, ctx.report_id, vf.usage_page, vf.usage
                );
                continue;
            }

            if vf.is_hat {
                // Normalize to slots
                let slot = hat_value_to_slot(
                    value as i32,
                    vf.logical_min,
                    vf.logical_max,
                    vf.hat_is_degrees,
                );
                if let Some(hidx) = vf.hat_index {
                    let last = self.last_hat_value.get(&hidx).copied().unwrap_or(i16::MIN);
                    if last != slot {
                        self.last_hat_value.insert(hidx, slot);
                        out.push(InputKind::HatChanged {
                            hat: hidx,
                            value: slot,
                        });
                    }
                }
            } else {
                // Axis normalize to [-1, 1]
                let v = normalize_axis_value(value as i32, vf.logical_min, vf.logical_max);
                if let Some(aidx) = vf.axis_index {
                    let last = self.last_axis_value.get(&aidx).copied().unwrap_or(f32::NAN);
                    if !last.is_finite() || (v - last).abs() > self.axis_epsilon {
                        self.last_axis_value.insert(aidx, v);
                        out.push(InputKind::AxisMoved {
                            axis: aidx,
                            value: v,
                        });
                    }
                }
            }
        }
    }
}

// --------------------- descriptor enumeration helpers ---------------------

/// Query HIDP for button capabilities for the given `report_type`.
///
/// Returns a vector sized to the number of caps returned by HIDP, or `None`
/// if the API reports failure.
fn enumerate_button_caps(
    ppd: PHIDP_PREPARSED_DATA,
    report_type: HIDP_REPORT_TYPE,
) -> Option<Vec<HIDP_BUTTON_CAPS>> {
    unsafe {
        let len: u16 = 64;
        let mut caps: Vec<HIDP_BUTTON_CAPS> = vec![
            HIDP_BUTTON_CAPS {
                ..core::mem::zeroed()
            };
            len as usize
        ];

        let mut needed: u16 = len;
        let status = HidP_GetButtonCaps(report_type, caps.as_mut_ptr(), &mut needed, ppd);

        if status == STATUS_SUCCESS {
            caps.truncate(needed as usize);
            Some(caps)
        } else {
            None
        }
    }
}

/// Query HIDP for value capabilities for the given `report_type`.
///
/// Returns a vector sized to the number of caps returned by HIDP, or `None`
/// if the API reports failure.
fn enumerate_value_caps(
    ppd: PHIDP_PREPARSED_DATA,
    report_type: HIDP_REPORT_TYPE,
) -> Option<Vec<HIDP_VALUE_CAPS>> {
    unsafe {
        let len: u16 = 64;
        let mut caps: Vec<HIDP_VALUE_CAPS> = vec![
            HIDP_VALUE_CAPS {
                ..core::mem::zeroed()
            };
            len as usize
        ];
        let mut needed: u16 = len;

        let status = HidP_GetValueCaps(report_type, caps.as_mut_ptr(), &mut needed, ppd);
        if status == STATUS_SUCCESS {
            caps.truncate(needed as usize);
            Some(caps)
        } else {
            None
        }
    }
}

/// Expand button caps (including usage ranges) into concrete button fields.
fn normalize_buttons(caps: &[HIDP_BUTTON_CAPS]) -> Vec<ButtonField> {
    let mut out = Vec::new();

    for c in caps {
        let rid = c.ReportID;
        let up = c.UsagePage;

        // Expand to concrete usages
        let mut usages = Vec::new();
        let is_range = c.IsRange != 0;
        unsafe {
            if is_range {
                let r = c.Anonymous.Range;
                let u_min = r.UsageMin;
                let u_max = r.UsageMax;
                if u_min <= u_max {
                    for u in u_min..=u_max {
                        usages.push(u);
                    }
                }
            } else {
                let nr = c.Anonymous.NotRange;
                usages.push(nr.Usage);
            }
        }

        out.push(ButtonField {
            report_id: rid,
            usage_page: up,
            usages,
        });
    }
    out
}

/// Expand value caps into per-usage value fields and classify hats.
fn normalize_values(caps: &[HIDP_VALUE_CAPS]) -> Vec<ValueField> {
    let mut out = Vec::new();
    for c in caps {
        let rid = c.ReportID;
        let up = c.UsagePage;
        let logical_min = c.LogicalMin as i32;
        let logical_max = c.LogicalMax as i32;

        let is_range = c.IsRange != 0;
        unsafe {
            if is_range {
                let r = c.Anonymous.Range;
                let u_min = r.UsageMin;
                let u_max = r.UsageMax;
                for u in u_min..=u_max {
                    let (is_hat, hat_is_degrees) = classify_hat(up, u, logical_min, logical_max);
                    out.push(ValueField {
                        report_id: rid,
                        usage_page: up,
                        usage: u,
                        logical_min,
                        logical_max,
                        is_hat,
                        hat_is_degrees,
                        axis_index: None,
                        hat_index: None,
                    });
                }
            } else {
                let nr = c.Anonymous.NotRange;
                let u = nr.Usage;
                let (is_hat, hat_is_degrees) = classify_hat(up, u, logical_min, logical_max);
                out.push(ValueField {
                    report_id: rid,
                    usage_page: up,
                    usage: u,
                    logical_min,
                    logical_max,
                    is_hat,
                    hat_is_degrees,
                    axis_index: None,
                    hat_index: None,
                });
            }
        }
    }
    out
}

/// Determine if a (usage_page, usage) is a Hat and whether it encodes degrees.
///
/// Returns `(is_hat, is_degrees)`. When `is_hat` is true:
/// - `is_degrees == false` means logical slots (e.g., `0..7` or `1..8`).
/// - `is_degrees == true` means an angular domain (e.g., `0..359`).
fn classify_hat(usage_page: u16, usage: u16, logical_min: i32, logical_max: i32) -> (bool, bool) {
    // Generic Desktop page, usage 0x39 = Hat Switch
    if usage_page == 0x01 && usage == 0x39 {
        // Use full logical range, not just max:
        // Treat as "slots" if the device describes exactly 8 positions (0..7 or 1..8).
        let is_slots =
            (logical_min == 0 && logical_max == 7) || (logical_min == 1 && logical_max == 8);
        // Otherwise assume degrees (0..359, 0..100, etc.)
        let is_degrees = !is_slots;
        (true, is_degrees)
    } else {
        (false, false)
    }
}

// --------------------- decoding helpers ---------------------

/// Normalize an integer axis value from `[lo..hi]` into `[-1.0, 1.0]` with clamping.
fn normalize_axis_value(v: i32, lo: i32, hi: i32) -> f32 {
    let lo = lo as f64;
    let hi = hi as f64;
    if (hi - lo).abs() < 1e-9 {
        return 0.0;
    }
    let t = (v as f64 - lo) / (hi - lo); // 0..1
    let n = t * 2.0 - 1.0; // -1..1
    n.clamp(-1.0, 1.0) as f32
}

/// Convert a raw hat value into a standardized slot:
/// - Returns `-1` for neutral.
/// - Returns `0..7` for directions (Up=0, clockwise).
/// - If `is_degrees`, maps degrees using 45° sectors; otherwise expects slots.
fn hat_value_to_slot(raw: i32, lo: i32, hi: i32, is_degrees: bool) -> i16 {
    // Unify common neutral encodings (outside logical range or special sentinels).
    if raw < lo || raw > hi || matches!(raw, -1 | 8 | 15 | 255 | 0xFFFF) {
        return -1;
    }
    if !is_degrees {
        // slots already
        if raw >= 0 && raw <= 7 {
            return raw as i16;
        }
        // unknown → neutral
        return -1;
    }
    // degrees → slot
    let deg = raw as f32;
    let mut slot = ((deg + 22.5) / 45.0).floor() as i32 % 8;
    if slot < 0 {
        slot += 8;
    }
    slot as i16
}

/// Provide friendly names for common Generic Desktop usages (X/Y/Z/Rx/Ry/Rz/etc.).
fn usage_name(usage_page: u16, usage: u16) -> Option<String> {
    // Minimal friendly labels for common axes; otherwise leave None.
    if usage_page == 0x01 {
        let s = match usage {
            0x30 => "X",
            0x31 => "Y",
            0x32 => "Z",
            0x33 => "Rx",
            0x34 => "Ry",
            0x35 => "Rz",
            0x36 => "Slider",
            0x37 => "Dial",
            0x38 => "Wheel",
            0x39 => "Hat",
            _ => return None,
        };
        return Some(s.to_string());
    }
    None
}

// --------------------- OS handle helpers ---------------------

/// Open a Windows file handle for a HID interface path.
///
/// ### Errors
/// Returns `Err(GetLastError())` on failure.
///
/// ### Safety
/// The returned `HANDLE` must be closed with `CloseHandle` when no longer used.
fn open_device_handle(path: &str) -> Result<HANDLE, u32> {
    // Convert to UTF-16 and NUL-terminate
    let wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // Try READ|WRITE first (preferred), then fall back to READ-only.
    let try_open = |access: u32| unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(), // lpSecurityAttributes
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0, // hTemplateFile
        )
    };

    let mut handle = try_open(GENERIC_READ | GENERIC_WRITE);
    if handle == INVALID_HANDLE_VALUE {
        // Some devices refuse write access; READ is enough for HIDP decode.
        handle = try_open(GENERIC_READ);
    }

    if handle == INVALID_HANDLE_VALUE {
        let code = unsafe { GetLastError() };
        Err(code)
    } else {
        Ok(handle)
    }
}
