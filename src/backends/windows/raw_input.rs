//! Windows Raw Input parsing helpers (keyboard + mouse).
//!
//! This module is intentionally "dumb": it only parses `WM_INPUT` payloads into
//! small structs. Higher-level routing (device registration, event enqueueing)
//! lives in `Manager`.
//!
//! ## What you get
//! - Keyboard packets: scancode + extended flag + break/make state
//! - Mouse packets: dx/dy deltas, wheel/hwheel deltas, and button flag bits
//!
//! ## What you **don't** get (by design)
//! - No device registration / stable device IDs (the manager decides that)
//! - No binding rules / transforms / smoothing
//! - No text/character translation (this is *not* a WM_CHAR layer)
//!
//! ## Conventions
//! - Mouse deltas are reported in **raw OS units** (counts) as provided by Raw Input.
//! - Wheel deltas are reported in **raw WHEEL_DELTA units** (typically ±120 per notch).
//! - Keyboard identity is represented as `(scancode, extended)` and can be packed into a `u16`
//!   via [`pack_key_index`]. This is intended for stable binding keys, not character mapping.

#![cfg(target_os = "windows")]

use core::ffi::c_void;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC_EX};
use windows_sys::Win32::UI::Input::*;
#[derive(Clone, Copy, Debug)]
pub(crate) struct RawKeyboardPacket {
    /// Raw Input device handle that produced the event.
    pub hdevice: HANDLE,
    /// Hardware scancode (layout-independent).
    pub scancode: u16,
    /// Extended key flag (E0/E1 or MapVirtualKey-derived).
    pub is_extended: bool,
    /// `true` for key-up (break), `false` for key-down (make).
    pub is_break: bool,
}
#[inline]
fn vkey_to_scancode(vkey: u16) -> Option<(u16, bool)> {
    unsafe {
        // MAPVK_VK_TO_VSC_EX can encode extended keys by returning 0xE0xx.
        let sc = MapVirtualKeyW(vkey as u32, MAPVK_VK_TO_VSC_EX) as u32;
        if sc == 0 {
            return None;
        }
        // If high byte is 0xE0, treat as extended and use low byte as scancode.
        if (sc & 0xFF00) == 0xE000 {
            Some(((sc & 0x00FF) as u16, true))
        } else {
            Some((sc as u16, false))
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct RawMousePacket {
    /// Raw Input device handle that produced the event.
    pub hdevice: HANDLE,
    /// Relative delta X (raw counts).
    pub dx: i32,
    /// Relative delta Y (raw counts).
    pub dy: i32,
    /// RAWMOUSE usButtonFlags bitfield (RI_MOUSE_*).
    pub buttons_flags: u16,
    pub _buttons_data: u16,
    /// Vertical wheel delta (typically ±120 per detent when present).
    pub wheel_delta: i16,
    /// Horizontal wheel delta (typically ±120 per detent when present).
    pub hwheel_delta: i16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RawInputPacket {
    Keyboard(RawKeyboardPacket),
    Mouse(RawMousePacket),
}

// Local constants (avoid relying on module exports that vary by windows-sys version)
const RI_KEY_BREAK: u16 = 0x0001;
const RI_KEY_E0: u16 = 0x0002;
const RI_KEY_E1: u16 = 0x0004;

const RI_MOUSE_WHEEL: u16 = 0x0400;
const RI_MOUSE_HWHEEL: u16 = 0x0800;

/// Parse a `WM_INPUT` lparam into a keyboard or mouse packet (if applicable).
pub(crate) fn read_wm_input(lparam: isize) -> Option<RawInputPacket> {
    unsafe {
        // Query size
        let mut size: u32 = 0;
        let r0 = GetRawInputData(
            lparam as _,
            RID_INPUT,
            core::ptr::null_mut(),
            &mut size,
            core::mem::size_of::<RAWINPUTHEADER>() as u32,
        );
        if r0 == u32::MAX || size == 0 {
            return None;
        }

        // Read buffer
        let mut buf = vec![0u8; size as usize];
        let r1 = GetRawInputData(
            lparam as _,
            RID_INPUT,
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
            core::mem::size_of::<RAWINPUTHEADER>() as u32,
        );
        if r1 == u32::MAX {
            return None;
        }

        read_raw_input_bytes(&buf)
    }
}
/// Parse a raw `RID_INPUT` payload (bytes returned by `GetRawInputData`) into a keyboard
/// or mouse packet (if applicable). This is safe to call later, as long as the bytes
/// were copied during `WM_INPUT`.
pub(crate) fn read_raw_input_bytes(buf: &[u8]) -> Option<RawInputPacket> {
    let hdr_sz = core::mem::size_of::<RAWINPUTHEADER>();
    if buf.len() < hdr_sz {
        return None;
    }

    unsafe {
        // Read header only (RAWINPUT payload is variable-sized: 40 for kbd, 48 for mouse, etc.)
        let hdr: RAWINPUTHEADER = core::ptr::read_unaligned(buf.as_ptr() as *const RAWINPUTHEADER);
        let data_ptr = buf.as_ptr().add(hdr_sz);

        match hdr.dwType {
            RIM_TYPEKEYBOARD => {
                let need = hdr_sz + core::mem::size_of::<RAWKEYBOARD>();
                if buf.len() < need {
                    return None;
                }

                let kbd: RAWKEYBOARD = core::ptr::read_unaligned(data_ptr as *const RAWKEYBOARD);
                let make: u16 = kbd.MakeCode as u16;
                let flags: u16 = kbd.Flags as u16;
                let vkey: u16 = kbd.VKey as u16;

                let is_break = (flags & RI_KEY_BREAK) != 0;
                let is_extended_flags = (flags & (RI_KEY_E0 | RI_KEY_E1)) != 0;

                // Prefer MakeCode; if it's 0, fallback from VKey.
                let (scancode, ext_from_map) = if make != 0 {
                    (make, false)
                } else if let Some((sc, ext)) = vkey_to_scancode(vkey) {
                    (sc, ext)
                } else {
                    return None;
                };

                Some(RawInputPacket::Keyboard(RawKeyboardPacket {
                    hdevice: hdr.hDevice,
                    scancode,
                    is_extended: is_extended_flags || ext_from_map,
                    is_break,
                }))
            }

            RIM_TYPEMOUSE => {
                let need = hdr_sz + core::mem::size_of::<RAWMOUSE>();
                if buf.len() < need {
                    return None;
                }

                let m: RAWMOUSE = core::ptr::read_unaligned(data_ptr as *const RAWMOUSE);

                let buttons_flags: u16 = m.Anonymous.Anonymous.usButtonFlags;
                let buttons_data: u16 = m.Anonymous.Anonymous.usButtonData;

                let wheel_delta = if (buttons_flags & RI_MOUSE_WHEEL) != 0 {
                    buttons_data as i16
                } else {
                    0
                };
                let hwheel_delta = if (buttons_flags & RI_MOUSE_HWHEEL) != 0 {
                    buttons_data as i16
                } else {
                    0
                };

                Some(RawInputPacket::Mouse(RawMousePacket {
                    hdevice: hdr.hDevice,
                    dx: m.lLastX,
                    dy: m.lLastY,
                    buttons_flags,
                    _buttons_data: buttons_data,
                    wheel_delta,
                    hwheel_delta,
                }))
            }

            _ => None,
        }
    }
}
/// RawInput device interface path for a given `hDevice` (RIDI_DEVICENAME).
pub(crate) fn device_name(hdev: HANDLE) -> Option<String> {
    unsafe {
        // Query required size (in WCHARs, including NUL).
        let mut size: u32 = 0;
        let r0 = GetRawInputDeviceInfoW(hdev, RIDI_DEVICENAME, core::ptr::null_mut(), &mut size);
        if r0 == u32::MAX || size == 0 {
            return None;
        }

        let mut wide: Vec<u16> = vec![0u16; size as usize];
        let r1 = GetRawInputDeviceInfoW(
            hdev,
            RIDI_DEVICENAME,
            wide.as_mut_ptr() as *mut c_void,
            &mut size,
        );
        if r1 == u32::MAX {
            return None;
        }

        while wide.last() == Some(&0) {
            wide.pop();
        }
        Some(String::from_utf16_lossy(&wide))
    }
}

/// Pack a keyboard key identity into a stable `u16` index.
///
/// Layout:
/// - low 15 bits = scancode
/// - high bit    = extended flag
///
/// This is intended for **stable bindings** (e.g. "key_1E" vs "key_E01D"), not for mapping to text.
#[inline]
pub(crate) fn pack_key_index(scancode: u16, is_extended: bool) -> u16 {
    let mut idx = scancode & 0x7FFF;
    if is_extended {
        idx |= 0x8000;
    }
    idx
}
