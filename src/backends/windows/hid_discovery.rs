//! Windows device discovery (HID + XInput).
//!
//! This module provides the Windows backend’s discovery pipeline:
//!
//! - Enumerate HID devices via `hidapi`
//! - Filter out obvious non-game controls (mouse/keyboard) and XInput HID-compat endpoints
//! - Construct a HIDP-driven parser (`WinHidpParser`) to decode reports consistently
//! - Wrap successfully parsed devices as [`HidInputDevice`]
//! - Add XInput controllers (0..4) as [`XInputDevice`] entries
//!
//! ## `probe_devices` vs `probe_devices_with_debug`
//! - [`probe_devices`] returns only successfully wrapped devices.
//! - [`probe_devices_with_debug`] additionally returns per-HID-entry debug records describing
//!   where each HID device was accepted/rejected or failed (filter → parser → wrapper).
//!
//! This debug path exists to support tooling (e.g. AxisMirror device reports) without changing
//! runtime behavior.

#![cfg(target_os = "windows")]

use crate::backends::windows::hid_device::HidInputDevice;
use crate::backends::windows::hidp_parser::WinHidpParser;
use crate::backends::windows::xinput_devices::XInputDevice;
use crate::device::{Device, DeviceFingerprint};
use crate::event::{ChannelDesc, ChannelKind};
use crate::metadata::DeviceMeta;
use hidapi::{DeviceInfo, HidApi};

/// High-level reason why we were unable to construct a HIDP parser.
///
/// At the moment this is coarse-grained (`Unknown`), but it gives us a clear
/// "parser failed vs filter failed vs wrap failed" split. We can refine this
/// later by plumbing more detailed error codes out of `WinHidpParser::new`.
#[derive(Debug, Clone)]
pub enum ParserFailKind {
    /// Construction of `WinHidpParser` returned `None` for an unspecified reason.
    Unknown,
}

/// Where along the discovery pipeline a device was rejected or failed.
#[derive(Debug, Clone)]
pub enum DropStage {
    /// Rejected by `accept_device` (usage_page/usage/XInput filter).
    FilterRejected,

    /// HIDP descriptor-driven parser failed to construct.
    ParserFailed(ParserFailKind),

    /// We were unable to open or wrap the device as a `Device`.
    ///
    /// This typically corresponds to `HidInputDevice::new` returning `None`.
    DeviceWrapFailed(String),
}

/// High-level summary of what channels we think the device exposes.
///
/// This is derived from `Device::describe()` for successfully wrapped devices.
/// It is intentionally coarse (axes/buttons/hats counts only). For devices
/// where the parser failed, this will be `None`.
#[derive(Debug, Clone)]
pub struct CapsSummary {
    pub axes: usize,
    pub buttons: usize,
    pub hats: usize,
    /// Report IDs are left empty for now because we derive this summary from
    /// `Device::describe()` instead of the raw HIDP caps. We can extend this
    /// in the future if we plumb more data out of `WinHidpParser`.
    pub report_ids: Vec<u8>,
    /// `true` if the device appears to expose only a single "report 0" in
    /// practice. Currently we default this to `false` because we do not query
    /// HIDP directly here.
    pub only_rid0: bool,
}

/// Debug view of a single HID entry from `HidApi::device_list()`.
///
/// This is intended for tooling (AxisMirror's device report generator) and
/// is not used in the main runtime path. It lets us answer:
/// - Did the device pass `accept_device`?
/// - Did the HIDP parser construct?
/// - Did we successfully wrap it as a `Device`?
/// - For successful devices, how many axes/buttons/hats did we see?
#[derive(Debug, Clone)]
pub struct DeviceDebugInfo {
    // Basic identity from hidapi:
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
    pub product_string: Option<String>,
    pub path: String,
    pub usage_page: Option<u16>,
    pub usage: Option<u16>,
    pub interface_number: i32,

    // Discovery decisions:
    /// Result of `accept_device(info)`.
    pub accepted_by_filter: bool,

    /// If `Some`, indicates where the device was dropped. `None` means the
    /// device was successfully wrapped and returned as a `Device`.
    pub drop_stage: Option<DropStage>,

    /// For successfully wrapped devices, a coarse summary of channels.
    /// `None` if parser/wrap failed.
    pub caps: Option<CapsSummary>,
}

/// Decide whether a `hidapi` device entry should be considered for wrapping.
///
/// Rules:
/// - Accept usage pages commonly used for game controls:
///   - `0x01` Generic Desktop
///   - `0x02` Simulation Controls
///   - `0x0F` Physical Interface
///   - `0xFFxx` Vendor-defined
/// - Reject plain mouse/keyboard endpoints on Generic Desktop.
/// - Reject XInput HID-compat “IG_” endpoints to avoid double-counting (XInput is added separately).
fn accept_device(info: &DeviceInfo) -> bool {
    let up = info.usage_page();
    let u = info.usage();

    // Allow the usage pages that commonly carry game controls:
    // - 0x01: Generic Desktop (sticks, gamepads, multi-axis, vJoy, etc.)
    // - 0x02: Simulation Controls (rudder/pedals on some devices)
    // - 0x0F: Physical Interface (some throttle/pedal stacks)
    // - 0xFFxx: Vendor-defined (Virpil/VPC virtual endpoints, etc.)
    let is_generic_desktop = up == 0x01;
    let is_simulation = up == 0x02;
    let is_physical_iface = up == 0x0F;
    let is_vendor_defined = (up & 0xFF00) == 0xFF00;

    if !(is_generic_desktop || is_simulation || is_physical_iface || is_vendor_defined) {
        return false;
    }

    // On Generic Desktop, throw away plain mouse / keyboard.
    // (Mouse = 0x02, Keyboard = 0x06 on Usage Page 0x01.)
    if is_generic_desktop && matches!(u, 0x02 | 0x06) {
        return false;
    }

    // ⬇️ Generic “this is an XInput HID compat endpoint, skip it”
    let path = info.path().to_string_lossy();
    if is_generic_desktop && u == 0x05 && path.contains("IG_") {
        // Generic Desktop Gamepad, Interface Group = XInput-style HID.
        // We already have it via XInput APIs, so don't double-count.
        return false;
    }

    // Everything else on Generic Desktop is allowed:
    // - Joystick (0x04)
    // - Gamepad (0x05)
    // - Multi-axis (0x08)
    // - And any other vendor-odd usages like Virpil pedals.
    true
}

/// Debug-aware variant of `probe_devices` that returns both the discovered
/// devices and a per-HID-entry debug record describing how each device fared
/// in the discovery pipeline.
///
/// This does **not** change the behavior of `probe_devices`; it simply
/// exposes the intermediate decisions to tooling (e.g., AxisMirror's device
/// report generator).
///
/// Note: XInput devices are still returned in the `devices` list (same as `probe_devices`),
/// but debug records are currently emitted only for `hidapi` device-list entries.
pub fn probe_devices_with_debug(api: &HidApi) -> (Vec<Box<dyn Device>>, Vec<DeviceDebugInfo>) {
    let mut devices: Vec<Box<dyn Device>> = Vec::new();
    let mut debug: Vec<DeviceDebugInfo> = Vec::new();

    // 1) HID devices
    for info in api.device_list() {
        let vid = info.vendor_id();
        let pid = info.product_id();
        let usage_page = Some(info.usage_page());
        let usage = Some(info.usage());
        let interface_number = info.interface_number();

        let mut dbg = DeviceDebugInfo {
            vid,
            pid,
            serial: info.serial_number().map(|s| s.to_string()),
            product_string: info.product_string().map(|s| s.to_string()),
            path: info.path().to_string_lossy().to_string(),
            usage_page,
            usage,
            interface_number,
            accepted_by_filter: false,
            drop_stage: None,
            caps: None,
        };

        // Filter by usage/IG_ rules.
        if !accept_device(info) {
            dbg.accepted_by_filter = false;
            dbg.drop_stage = Some(DropStage::FilterRejected);
            debug.push(dbg);
            continue;
        }
        dbg.accepted_by_filter = true;

        // Attempt to build the HIDP parser.
        let parser = match WinHidpParser::new(info) {
            Some(p) => p,
            None => {
                dbg.drop_stage = Some(DropStage::ParserFailed(ParserFailKind::Unknown));
                debug.push(dbg);
                continue;
            }
        };

        // Attempt to wrap as a HidInputDevice.
        match HidInputDevice::new(info, api, parser, fingerprint(info), meta(info)) {
            Some(dev) => {
                // Derive a coarse caps summary from Device::describe().
                let descs: Vec<ChannelDesc> = dev.describe();
                let mut axes = 0usize;
                let mut buttons = 0usize;
                let mut hats = 0usize;
                for ch in &descs {
                    match ch.kind {
                        ChannelKind::Axis => axes += 1,
                        ChannelKind::Button => buttons += 1,
                        ChannelKind::Hat => hats += 1,
                    }
                }

                dbg.caps = Some(CapsSummary {
                    axes,
                    buttons,
                    hats,
                    report_ids: Vec::new(), // not derived here (HIDP-internal)
                    only_rid0: false,       // not derived here
                });
                dbg.drop_stage = None;
                devices.push(Box::new(dev) as Box<dyn Device>);
                debug.push(dbg);
            }
            None => {
                dbg.drop_stage = Some(DropStage::DeviceWrapFailed(
                    "HidInputDevice::new returned None".into(),
                ));
                debug.push(dbg);
            }
        }
    }

    // 2) XInput devices (same behavior as probe_devices; no debug records yet).
    for index in 0..4 {
        let fp = DeviceFingerprint {
            vendor_id: 0x045e,
            product_id: 0x0000,
            serial_number: Some(format!("xinput:{index}")),
            path: Some(format!("xinput:{index}")),
        };

        let meta = DeviceMeta {
            bus: Some("xinput".into()),
            vid: Some(0x045e),
            pid: Some(0x0000),
            product_string: Some(format!("XInput Controller {}", index)),
            serial_number: Some(format!("xinput:{index}")),
            usage_page: None,
            usage: None,
            interface_number: None,
            container_id: None,
            path: Some(format!("xinput:{index}")),
        };

        let dev = XInputDevice::new(index, fp, meta);
        devices.push(Box::new(dev) as Box<dyn Device>);
    }

    (devices, debug)
}

fn fingerprint(info: &DeviceInfo) -> DeviceFingerprint {
    DeviceFingerprint {
        vendor_id: info.vendor_id(),
        product_id: info.product_id(),
        serial_number: info.serial_number().map(|s| s.to_owned()),
        path: Some(info.path().to_string_lossy().to_string()),
    }
}

/// Build a lightweight [`DeviceMeta`] snapshot for a `hidapi` device entry.
///
/// Fields are best-effort; unknown values remain `None`.
fn meta(info: &DeviceInfo) -> DeviceMeta {
    let interface_number = {
        let n = info.interface_number();
        if n >= 0 {
            Some(n)
        } else {
            None
        }
    };
    DeviceMeta {
        bus: Some("usb".into()),
        vid: Some(info.vendor_id()),
        pid: Some(info.product_id()),
        product_string: info.product_string().map(|s| s.to_string()),
        serial_number: info.serial_number().map(|s| s.to_string()),
        usage_page: Some(info.usage_page()),
        usage: Some(info.usage()),
        interface_number,
        container_id: None,
        path: Some(info.path().to_string_lossy().to_string()),
    }
}

/// Discover all enabled Windows devices and wrap them as [`Device`] trait objects.
///
/// - HID devices are enumerated via `hidapi`, filtered with [`accept_device`], and require
///   successful construction of a HIDP parser (`WinHidpParser`) to be included.
/// - XInput controllers are added as up to 4 synthetic device entries (`xinput:0..3`).
///
/// This function returns only successfully wrapped devices (no debug records).
pub fn probe_devices(api: &HidApi) -> Vec<Box<dyn Device>> {
    let mut out: Vec<Box<dyn Device>> = Vec::new();

    // 1) HID devices (what you already had)
    for info in api.device_list() {
        if !accept_device(info) {
            continue;
        }

        // HIDP parser is mandatory. If it fails, skip the device.
        if let Some(parser) = WinHidpParser::new(info) {
            if let Some(dev) = HidInputDevice::new(info, api, parser, fingerprint(info), meta(info))
            {
                out.push(Box::new(dev));
            }
        }
    }

    // 2) XInput devices (up to 4 slots)
    {
        for index in 0..4 {
            // Build a synthetic fingerprint & meta for this virtual device.
            // Main thing: serial/path are unique per slot so your existing
            // device_uid() logic has something to chew on.
            //
            // Note: this is *not* virtual-device generation. It is a wrapper over the Windows
            // XInput API surfaced through the same `Device` trait.

            let fp = DeviceFingerprint {
                vendor_id: 0x045e,  // Microsoft (arbitrary but reasonable)
                product_id: 0x0000, // "generic XInput" - you can pick any
                serial_number: Some(format!("xinput:{index}")),
                path: Some(format!("xinput:{index}")),
            };

            let meta = DeviceMeta {
                bus: Some("xinput".into()),
                vid: Some(0x045e),
                pid: Some(0x0000),
                product_string: Some(format!("XInput Controller {}", index)),
                serial_number: Some(format!("xinput:{index}")),
                usage_page: None,
                usage: None,
                interface_number: None,
                container_id: None,
                path: Some(format!("xinput:{index}")),
            };

            let dev = XInputDevice::new(index, fp, meta);
            out.push(Box::new(dev) as Box<dyn Device>);
        }
    }

    out
}
