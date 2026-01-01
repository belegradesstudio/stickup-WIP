#![cfg(target_os = "windows")]
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
//! Hats are normalized to **slots**:
//! - `-1` = neutral
//! - `0..7` = directions (Up=0, clockwise)
//!
//! Devices that report **degrees** are converted to **slots** using 45° sectors:
//! `slot = floor((deg + 22.5) / 45) mod 8`. Neutral is reported as `-1`.
//!
//! ## Notes
//! - We open an OS handle from the HID path and keep it alive alongside the
//!   `PreparsedData` handle for the parser lifetime.
//! - If HIDP calls fail during construction, fall back to your generic parser.
//!
//! ## Dependencies
//! Requires `windows-sys` with HID + FileSystem features (see Cargo.toml).
//!
//! ## Platform & feature gates
//! - Windows only (`#![cfg(windows)]`).
//! - Consumes HID report **descriptors** at init and raw **reports** at `parse()`.
//!
//! ## Example (conceptual)
//! ```ignore
//! use stickup::backends::windows::hidp_parser::WinHidpParser;
//! use stickup::device::{ReportParser, ParseCtx};
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

use crate::device::{ParseCtx, ReportParser};

use crate::event::{ChannelDesc, ChannelKind, InputKind};

use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, NTSTATUS,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

const USE_LINK_COLLECTION_FOR_VALUES: bool = true;

const STATUS_SUCCESS: NTSTATUS = HIDP_STATUS_SUCCESS; // alias for clarity
const STATUS_BUFFER_TOO_SMALL: NTSTATUS = HIDP_STATUS_BUFFER_TOO_SMALL;

/// One normalized button field (expanded per usage).
#[derive(Clone, Debug)]
struct ButtonField {
    report_id: u8,
    usage_page: u16,
    link_collection: u16,
    // The concrete usage codes this cap covers (expanded from range where needed).
    usages: Vec<u16>,
}

/// One normalized value (axis/hat) field.
#[derive(Clone, Debug)]
struct ValueField {
    report_id: u8,
    usage_page: u16,
    usage: u16,
    link_collection: u16,
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
    // (report_id, usage_page, usage, link_collection) → idx
    button_index_by_usage: HashMap<(u8, u16, u16, u16), u16>,
    // Deterministic enumeration of buttons by assigned index:
    // each entry is (report_id, usage_page, usage, link_collection)
    buttons_by_index: Vec<(u8, u16, u16, u16)>,
    axis_fields_by_index: Vec<usize>, // axis_index → values[] idx
    hat_fields_by_index: Vec<usize>,  // hat_index → values[] idx

    // Last-frame state for edge/coalesce
    last_pressed_buttons: HashSet<u16>, // button indices currently pressed
    last_axis_value: HashMap<u16, f32>, // axis_index → last value
    last_hat_value: HashMap<u16, i16>,  // hat_index → last slot value
    axis_epsilon: f32,

    // gamepad support
    only_rid0: bool, // true if descriptor uses only report ID 0
}

impl Drop for WinHidpParser {
    fn drop(&mut self) {
        unsafe {
            if self.ppd != 0 {
                HidD_FreePreparsedData(self.ppd);
                self.ppd = 0;
            }
            if !self.handle.is_null() {
                CloseHandle(self.handle);
                self.handle = std::ptr::null_mut();
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

        // Enumerate button/value caps (input report).
        // It is legal for a device to have only axes (no buttons) or only
        // buttons (no axes), so treat individual caps failures as "none"
        // rather than a hard parser failure. We only bail if *both* are
        // effectively empty.
        let btn_caps_opt = enumerate_button_caps(ppd, HidP_Input);
        let val_caps_opt = enumerate_value_caps(ppd, HidP_Input);

        if btn_caps_opt.is_none() {
            #[cfg(feature = "debug-log")]
            eprintln!(
                "[HIDP/CAPS] GetButtonCaps failed; continuing without buttons (vid=0x{:04x} pid=0x{:04x})",
                info.vendor_id(),
                info.product_id()
            );
        }
        if val_caps_opt.is_none() {
            #[cfg(feature = "debug-log")]
            eprintln!(
                "[HIDP/CAPS] GetValueCaps failed; continuing without axes (vid=0x{:04x} pid=0x{:04x})",
                info.vendor_id(),
                info.product_id()
            );
        }

        let btn_caps = btn_caps_opt.unwrap_or_default();
        let val_caps = val_caps_opt.unwrap_or_default();

        if btn_caps.is_empty() && val_caps.is_empty() {
            unsafe {
                HidD_FreePreparsedData(ppd);
                CloseHandle(handle);
            }
            return None;
        }

        // ---- gamepad support: report IDs ----
        let mut report_ids = BTreeSet::new();
        for c in &btn_caps {
            report_ids.insert(c.ReportID);
        }
        for c in &val_caps {
            report_ids.insert(c.ReportID);
        }

        // true if *every* cap uses ReportID == 0 → single-report device, no ID byte
        let only_rid0 = report_ids.len() == 1 && report_ids.contains(&0);

        // (Optional debug)
        #[cfg(feature = "debug-log")]
        eprintln!(
            "[HIDP/REPORTS] vid=0x{:04x} pid=0x{:04x} report_ids={:?} only_rid0={}",
            info.vendor_id(),
            info.product_id(),
            report_ids,
            only_rid0
        );

        // Normalize caps → fields
        let buttons = normalize_buttons(&btn_caps);
        let mut values = normalize_values(&val_caps);

        // Device quirk: VKB T-Rudder tends to work only with LinkCollection = 0.
        if info.vendor_id() == 0x231d && info.product_id() == 0x011f {
            for v in &mut values {
                v.link_collection = 0;
            }
            #[cfg(feature = "debug-log")]
            eprintln!("[HIDP/QUIRK] Forced LinkCollection=0 for VKB T-Rudder (231d:011f)");
        }

        // Assign stable indices for axes/hats/buttons
        let mut axis_fields_by_index = Vec::new();
        let mut hat_fields_by_index = Vec::new();
        let mut button_index_by_usage: HashMap<(u8, u16, u16, u16), u16> = HashMap::new();
        let mut buttons_by_index: Vec<(u8, u16, u16, u16)> = Vec::new();

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
                    let key = (bf.report_id, bf.usage_page, u, bf.link_collection);
                    button_index_by_usage.insert(key, next_btn);
                    buttons_by_index.push(key);
                    next_btn += 1;
                }
            }
        }

        // DEBUG: summarize discovered report IDs for visibility during dev builds
        {
            use std::collections::BTreeSet;
            let mut rid_axes: BTreeSet<u8> = BTreeSet::new();
            let mut rid_hats: BTreeSet<u8> = BTreeSet::new();
            let mut rid_buttons: BTreeSet<u8> = BTreeSet::new();
            for v in &values {
                if v.is_hat {
                    rid_hats.insert(v.report_id);
                } else {
                    rid_axes.insert(v.report_id);
                }
            }
            for b in &buttons {
                rid_buttons.insert(b.report_id);
            }
            #[cfg(feature = "debug-log")]
            eprintln!(
                "[HIDP/MAP] vid=0x{:04x} pid=0x{:04x} axes_rids={:?} hats_rids={:?} btn_rids={:?}",
                info.vendor_id(),
                info.product_id(),
                rid_axes,
                rid_hats,
                rid_buttons
            );
        }

        // Derive an LSB-sized epsilon from the widest logical range.
        let mut max_span: i32 = 1;
        for v in &values {
            max_span = max_span.max(v.logical_max - v.logical_min);
        }
        let lsb = 2.0f32 / (max_span.max(1) as f32); // [-1..1] range → 1 LSB
        let axis_epsilon = lsb * 2.0; // ~2 LSBs to suppress jitter

        Some(Self {
            handle,
            ppd,
            input_report_max_len: caps.InputReportByteLength,
            buttons,
            values,
            button_index_by_usage,
            buttons_by_index,
            axis_fields_by_index,
            hat_fields_by_index,
            last_pressed_buttons: HashSet::new(),
            last_axis_value: HashMap::new(),
            last_hat_value: HashMap::new(),
            axis_epsilon,

            // gamepad support
            only_rid0,
        })
    }
}

// We only ever use the parser from a single device thread; the raw OS handles
// are opaque, and the type does not share internal references to Rust data.
// Marking Send is OK for our usage pattern.
unsafe impl Send for WinHidpParser {}

impl ReportParser for WinHidpParser {
    fn input_report_len(&self) -> Option<usize> {
        Some(self.input_report_max_len as usize)
    }

    /// Whether the upstream reader should treat the first byte as a Report ID prefix.
    ///
    /// If this device’s descriptor indicates it uses only ReportID==0 (`only_rid0`),
    /// then HIDP expects the buffer *without* an ID prefix.
    fn expects_report_id_prefix(&self) -> bool {
        !self.only_rid0
    }

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
        // Buttons — deterministic order (by assigned index)
        for (idx, &(_rid, up, u, _lc)) in self.buttons_by_index.iter().enumerate() {
            let idx = idx as u16;
            out.push(ChannelDesc {
                kind: ChannelKind::Button,
                idx,
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
        // Build a HIDP-compatible report buffer sized to InputReportByteLength:
        // [effective_report_id][body... padded ...]
        let max = self.input_report_max_len as usize;
        let mut report = vec![0u8; max.max(1)];

        // If the descriptor says "only report ID 0" but ctx.report_id != 0,
        // then that first byte is almost certainly *data*, not an ID.
        // In that case interpret (ctx.report_id + payload) as the report body
        // and use effective_rid = 0 for HIDP.
        let mut tmp_body: Vec<u8>;
        let (effective_rid, body): (u8, &[u8]) = if self.only_rid0 && ctx.report_id != 0 {
            tmp_body = Vec::with_capacity(1 + payload.len());
            tmp_body.push(ctx.report_id);
            tmp_body.extend_from_slice(payload);
            (0, tmp_body.as_slice())
        } else {
            (ctx.report_id, payload)
        };

        report[0] = effective_rid;
        let copy_len = body.len().min(report.len().saturating_sub(1));
        report[1..1 + copy_len].copy_from_slice(&body[..copy_len]);
        let report_len_full = report.len() as u32;

        // ----- BUTTONS -----
        let mut pressed_now: BTreeSet<u16> = BTreeSet::new();

        for bf in &self.buttons {
            // IMPORTANT: filter on *effective* report ID, not raw ctx.report_id
            if bf.report_id != 0 && bf.report_id != effective_rid {
                continue;
            }

            let mut usage_buf = [0u16; 128];
            let mut usage_len: u32 = usage_buf.len() as u32;

            let status = unsafe {
                HidP_GetUsages(
                    HidP_Input,
                    bf.usage_page,
                    bf.link_collection,
                    usage_buf.as_mut_ptr(),
                    &mut usage_len,
                    self.ppd,
                    report.as_mut_ptr(),
                    report_len_full,
                )
            };

            if status != STATUS_SUCCESS {
                #[cfg(feature = "debug-log")]
                eprintln!(
                    "[HIDP] GetUsages failed: status=0x{:08x} rid={} up=0x{:02x}",
                    status as u32, effective_rid, bf.usage_page
                );
                continue;
            }

            for i in 0..(usage_len as usize) {
                let usage = usage_buf[i];
                // Prefer exact RID; fall back to RID=0 for stacks whose caps report 0.
                let key_exact = (effective_rid, bf.usage_page, usage, bf.link_collection);
                let key_fallback = (0, bf.usage_page, usage, bf.link_collection);
                if let Some(&btn_idx) = self
                    .button_index_by_usage
                    .get(&key_exact)
                    .or_else(|| self.button_index_by_usage.get(&key_fallback))
                {
                    pressed_now.insert(btn_idx);
                }
            }
        }

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

        // ----- VALUES (axes + hats) -----
        for vf in self.values.iter_mut() {
            if vf.report_id != 0 && vf.report_id != effective_rid {
                continue;
            }

            let mut value: u32 = 0;
            let lc = if USE_LINK_COLLECTION_FOR_VALUES {
                vf.link_collection
            } else {
                0
            };

            let mut status = unsafe {
                HidP_GetUsageValue(
                    HidP_Input,
                    vf.usage_page,
                    lc,
                    vf.usage as u16,
                    &mut value,
                    self.ppd,
                    report.as_mut_ptr(),
                    report_len_full,
                )
            };

            if status != STATUS_SUCCESS && lc != 0 {
                let retry_status = unsafe {
                    HidP_GetUsageValue(
                        HidP_Input,
                        vf.usage_page,
                        0,
                        vf.usage as u16,
                        &mut value,
                        self.ppd,
                        report.as_mut_ptr(),
                        report_len_full,
                    )
                };
                if retry_status == STATUS_SUCCESS {
                    vf.link_collection = 0;
                    status = STATUS_SUCCESS;
                    #[cfg(feature = "debug-log")]
                    eprintln!(
                    "[HIDP/QUIRK] rid={} up=0x{:02x} u=0x{:02x} forced LinkCollection=0 (runtime)",
                    effective_rid,
                    vf.usage_page,
                    vf.usage
                );
                }
            }

            if status != STATUS_SUCCESS {
                #[cfg(feature = "debug-log")]
                eprintln!(
                    "[HIDP] GetUsageValue failed: status=0x{:08x} rid={} up=0x{:02x} u=0x{:02x}",
                    status as u32, effective_rid, vf.usage_page, vf.usage
                );
                continue;
            }

            // rest of axis / hat logic unchanged...

            if vf.is_hat {
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
        // First try with a reasonable default buffer.
        let mut len: u16 = 64;
        let mut caps: Vec<HIDP_BUTTON_CAPS> = vec![
            HIDP_BUTTON_CAPS {
                ..core::mem::zeroed()
            };
            len as usize
        ];

        let mut needed: u16 = len;
        let mut status = HidP_GetButtonCaps(report_type, caps.as_mut_ptr(), &mut needed, ppd);

        if status == STATUS_SUCCESS {
            caps.truncate(needed as usize);
            return Some(caps);
        }

        // If the buffer was too small, retry with the required size.
        if status == STATUS_BUFFER_TOO_SMALL && needed > 0 {
            len = needed;
            let mut caps2: Vec<HIDP_BUTTON_CAPS> = vec![
                HIDP_BUTTON_CAPS {
                    ..core::mem::zeroed()
                };
                len as usize
            ];
            let mut needed2: u16 = len;
            status = HidP_GetButtonCaps(report_type, caps2.as_mut_ptr(), &mut needed2, ppd);

            if status == STATUS_SUCCESS {
                caps2.truncate(needed2 as usize);
                return Some(caps2);
            }
        }

        None
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
        // First try with a reasonable default buffer.
        let mut len: u16 = 64;
        let mut caps: Vec<HIDP_VALUE_CAPS> = vec![
            HIDP_VALUE_CAPS {
                ..core::mem::zeroed()
            };
            len as usize
        ];
        let mut needed: u16 = len;

        let mut status = HidP_GetValueCaps(report_type, caps.as_mut_ptr(), &mut needed, ppd);
        if status == STATUS_SUCCESS {
            caps.truncate(needed as usize);
            return Some(caps);
        }

        // If the buffer was too small, retry with the required size.
        if status == STATUS_BUFFER_TOO_SMALL && needed > 0 {
            len = needed;
            let mut caps2: Vec<HIDP_VALUE_CAPS> = vec![
                HIDP_VALUE_CAPS {
                    ..core::mem::zeroed()
                };
                len as usize
            ];
            let mut needed2: u16 = len;

            status = HidP_GetValueCaps(report_type, caps2.as_mut_ptr(), &mut needed2, ppd);
            if status == STATUS_SUCCESS {
                caps2.truncate(needed2 as usize);
                return Some(caps2);
            }
        }

        None
    }
}

/// Expand button caps (including usage ranges) into concrete button fields.
fn normalize_buttons(caps: &[HIDP_BUTTON_CAPS]) -> Vec<ButtonField> {
    let mut out = Vec::new();

    for c in caps {
        let rid = c.ReportID;
        let up = c.UsagePage;
        let lc = c.LinkCollection;
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
            link_collection: lc,
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

        // Only reject true garbage (usage_page==0).
        // Usage==0 is valid on Simulation Controls and vendor collections.
        let push_field = |u: u16, out: &mut Vec<ValueField>| {
            if up == 0 {
                return;
            }
            let (is_hat, hat_is_degrees) = classify_hat(up, u, logical_min, logical_max);
            out.push(ValueField {
                report_id: rid,
                usage_page: up,
                usage: u,
                link_collection: c.LinkCollection,
                logical_min,
                logical_max,
                is_hat,
                hat_is_degrees,
                axis_index: None,
                hat_index: None,
            });
        };

        let is_range = c.IsRange != 0;
        unsafe {
            if is_range {
                let r = c.Anonymous.Range;
                let u_min = r.UsageMin;
                let u_max = r.UsageMax;
                for u in u_min..=u_max {
                    push_field(u, &mut out);
                }
            } else {
                let nr = c.Anonymous.NotRange;
                push_field(nr.Usage, &mut out);
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
    // Generic Desktop
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
            _ => return Some(format!("GD_{usage:#04x}")),
        };
        return Some(s.to_string());
    }

    // Simulation Controls
    if usage_page == 0x02 {
        let s = match usage {
            0x00 => "SimAxis",
            0xB0 => "Accelerator",
            0xB1 => "Brake",
            0xB2 => "Clutch",
            0xBB => "Throttle",
            _ => "Sim",
        };
        return Some(s.to_string());
    }

    // Vendor-defined
    if (usage_page & 0xFF00) == 0xFF00 {
        return Some("VendorAxis".into());
    }

    // Fallback
    Some(format!("UP_{usage_page:04x}_U_{usage:04x}"))
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
    use std::ptr::{null, null_mut};

    // UTF-16 + NUL
    let wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // Correct: CreateFileW has **7** params.
    let try_open = |access: u32| unsafe {
        CreateFileW(
            wide.as_ptr(),                      // lpFileName: PCWSTR
            access,                             // dwDesiredAccess
            FILE_SHARE_READ | FILE_SHARE_WRITE, // dwShareMode
            null(),                             // lpSecurityAttributes: *const SECURITY_ATTRIBUTES
            OPEN_EXISTING,                      // dwCreationDisposition
            FILE_ATTRIBUTE_NORMAL,              // dwFlagsAndAttributes
            null_mut(),                         // hTemplateFile: HANDLE
        )
    };

    let mut handle = try_open(GENERIC_READ | GENERIC_WRITE);
    if handle == INVALID_HANDLE_VALUE {
        handle = try_open(GENERIC_READ);
    }

    if handle == INVALID_HANDLE_VALUE {
        let code = unsafe { GetLastError() };
        Err(code)
    } else {
        Ok(handle)
    }
}
