#![cfg(target_os = "windows")]

//! Windows XInput device wrapper.
//!
//! This module exposes gamepads/controllers available through the Windows **XInput** API
//! as [`Device`](crate::device::Device) implementations.
//!
//! # Why XInput exists alongside HID
//! Many Xbox-style controllers show up as both:
//! - a HID interface (visible via `hidapi`), and
//! - an XInput slot (0–3) via `XInputGetState`.
//!
//! StickUp typically prefers XInput for those devices because it provides:
//! - stable slot-based polling (`0..4` devices),
//! - standardized button naming/behavior,
//! - and avoids double-counting when the HID path is an XInput compatibility endpoint.
//!
//! # Channel conventions
//! This device emits [`InputKind`](crate::event::InputKind) in StickUp’s standard format:
//! - Axes are normalized to `[-1.0, 1.0]`
//! - Buttons emit edge events (`Pressed` / `Released`)
//! - DPad emits a single hat value `-1 | 0..7` (Up=0, clockwise)
//!
//! ## Axes (6)
//! The axis indices are stable and intended for bindings/UI:
//! - `0`: Left stick X (LX)
//! - `1`: Left stick Y (LY) **inverted** (up = -1, down = +1)
//! - `2`: Right stick X (RX)
//! - `3`: Right stick Y (RY) **inverted**
//! - `4`: Left trigger (LT) mapped to `[-1..1]`
//! - `5`: Right trigger (RT) mapped to `[-1..1]`
//!
//! ## Hat (1)
//! - `hat = 0` uses the same 8-way convention as HID hats:
//!   `-1` neutral, `0..7` directions (Up=0 clockwise).
//!
//! # Debug logging
//! When compiled with the `debug-log` feature, the first successful poll after a
//! disconnect/reconnect logs a `[XINPUT/CONNECT]` line including the device id/fingerprint.
//!
//! # Limitations
//! - XInput does not expose full HID descriptors, so [`Device::describe`] is currently a stub
//!   in this module. If your UI depends on channel descriptions, fill it in with stable
//!   `ChannelDesc` entries matching the axis/button/hat indices documented above.

use crate::device::{Device, DeviceFingerprint};
use crate::event::{ChannelDesc, ChannelKind, InputKind};
use crate::metadata::DeviceMeta;

use std::time::Instant;

// Windows XInput FFI.
use windows_sys::Win32::UI::Input::XboxController::*;

/// Number of axes emitted for XInput devices.
///
/// Indices are documented in the module-level docs.
const MAX_AXES: usize = 6;

/// XInput-backed device (slot 0–3).
///
/// The stable device id returned by [`Device::id`] is derived from the provided
/// [`DeviceFingerprint`]. In StickUp’s Windows backend, these are typically
/// synthesized as `xinput:{slot}` fingerprints.
pub struct XInputDevice {
    /// XInput slot index in `0..4`.
    index: u32,
    /// Stable identity data for persistence and logging.
    fingerprint: DeviceFingerprint,
    /// Cached `fingerprint.to_string()` used as the device id.
    fingerprint_str: String,
    /// User-facing label (best-effort).
    name: String,
    /// Metadata snapshot exposed to callers.
    meta: DeviceMeta,
    /// Last known normalized axis values.
    last_axes: [f32; MAX_AXES],
    /// Last known button bitfield.
    last_buttons: u16,
    /// Last known hat value (-1 or 0..7).
    last_hat: i8,
    /// Timestamp of last successful poll.
    last_poll: Instant,
    /// Tracks whether we were previously connected (used for connect logging).
    connected: bool,
}

impl XInputDevice {
    /// Create a new XInput device wrapper for a given slot (0–3).
    ///
    /// `fingerprint` + `meta` are built by the backend discovery layer, similar to HID devices.
    ///
    /// A typical fingerprint is a synthesized identity like:
    /// - `vendor_id = 0x045e` (Microsoft, conventional)
    /// - `serial_number = Some("xinput:{slot}")`
    /// - `path = Some("xinput:{slot}")`
    pub fn new(index: u32, fingerprint: DeviceFingerprint, meta: DeviceMeta) -> Self {
        let name = format!("XInput Controller {}", index);

        Self {
            index,
            fingerprint_str: fingerprint.to_string(),
            fingerprint,
            name,
            meta,
            last_axes: [0.0; MAX_AXES],
            last_buttons: 0,
            last_hat: -1,
            last_poll: Instant::now(),
            connected: false,
        }
    }

    #[inline]
    /// Normalize a signed thumbstick axis into `[-1, 1]`.
    fn normalize_thumb(v: i16) -> f32 {
        // Map [-32768, 32767] -> [-1, 1]
        if v >= 0 {
            (v as f32) / 32767.0
        } else {
            (v as f32) / 32768.0
        }
    }

    #[inline]
    /// Normalize an 8-bit trigger into `[-1, 1]`.
    ///
    /// XInput reports triggers as `0..255`. StickUp maps this to:
    /// - `0   -> -1.0` (released)
    /// - `255 -> +1.0` (fully pressed)
    fn normalize_trigger(v: u8) -> f32 {
        // 0 -> -1.0 (released), 255 -> +1.0 (fully pressed)
        (v as f32) / 255.0 * 2.0 - 1.0
    }

    /// Map XInput DPad bits to your hat convention:
    ///
    /// -1 = neutral
    ///  0 = up
    ///  1 = up-right
    ///  2 = right
    ///  3 = down-right
    ///  4 = down
    ///  5 = down-left
    ///  6 = left
    ///  7 = up-left
    fn compute_hat(buttons: u16) -> i8 {
        let up = buttons & XINPUT_GAMEPAD_DPAD_UP != 0;
        let down = buttons & XINPUT_GAMEPAD_DPAD_DOWN != 0;
        let left = buttons & XINPUT_GAMEPAD_DPAD_LEFT != 0;
        let right = buttons & XINPUT_GAMEPAD_DPAD_RIGHT != 0;

        if !up && !down && !left && !right {
            return -1;
        }

        match (up, down, left, right) {
            (true, false, false, false) => 0, // U
            (true, false, false, true) => 1,  // UR
            (false, false, false, true) => 2, // R
            (false, true, false, true) => 3,  // DR
            (false, true, false, false) => 4, // D
            (false, true, true, false) => 5,  // DL
            (false, false, true, false) => 6, // L
            (true, false, true, false) => 7,  // UL
            // Conflicting stuff (up+down, left+right) -> neutral
            _ => -1,
        }
    }
}

impl Device for XInputDevice {
    /// Poll XInput for the current state and emit deltas as [`InputKind`] events.
    ///
    /// - If the slot is disconnected, returns an empty vec.
    /// - Axes are emitted when they change beyond a small epsilon.
    /// - Buttons emit edge events when bits flip.
    /// - DPad emits a hat change on transitions.
    fn poll(&mut self) -> Vec<InputKind> {
        let mut events = Vec::new();

        // FFI struct: must be manually zeroed
        let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };

        // NOTE: XInputGetState returns 0 on success.
        let res = unsafe { XInputGetState(self.index, &mut state) };

        if res != 0 {
            // Disconnected or empty slot.
            if self.connected {
                // You *could* emit "everything released" here if you want.
                self.connected = false;
            }
            return events;
        }
        let was_connected = self.connected;

        self.connected = true;
        self.last_poll = Instant::now();

        if !was_connected {
            #[cfg(feature = "debug-log")]
            eprintln!(
                "[XINPUT/CONNECT] slot={} id={} fp={}",
                self.index,
                self.fingerprint_str,
                self.fingerprint.to_string()
            );
        }

        let gp = state.Gamepad;

        // Axes: 0..3 sticks, 4..5 triggers
        let new_axes = [
            Self::normalize_thumb(gp.sThumbLX),
            // Invert Y to match “up = -1, down = +1” if that's how HID is normalized:
            -Self::normalize_thumb(gp.sThumbLY),
            Self::normalize_thumb(gp.sThumbRX),
            -Self::normalize_thumb(gp.sThumbRY),
            Self::normalize_trigger(gp.bLeftTrigger),
            Self::normalize_trigger(gp.bRightTrigger),
        ];

        for (i, &v) in new_axes.iter().enumerate() {
            if (v - self.last_axes[i]).abs() > 0.001 {
                self.last_axes[i] = v;
                events.push(InputKind::AxisMoved {
                    axis: i as u16,
                    value: v,
                });
            }
        }

        // Buttons
        let buttons: u16 = gp.wButtons;
        let changed = buttons ^ self.last_buttons;

        // Map XInput buttons to stickup button indices.
        // These indices are arbitrary, just keep them stable.
        const BUTTON_MAP: &[(u16, u8)] = &[
            (XINPUT_GAMEPAD_A, 0),
            (XINPUT_GAMEPAD_B, 1),
            (XINPUT_GAMEPAD_X, 2),
            (XINPUT_GAMEPAD_Y, 3),
            (XINPUT_GAMEPAD_LEFT_SHOULDER, 4),
            (XINPUT_GAMEPAD_RIGHT_SHOULDER, 5),
            (XINPUT_GAMEPAD_BACK, 6),
            (XINPUT_GAMEPAD_START, 7),
            (XINPUT_GAMEPAD_LEFT_THUMB, 8),
            (XINPUT_GAMEPAD_RIGHT_THUMB, 9),
        ];

        for &(mask, idx) in BUTTON_MAP {
            if changed & mask != 0 {
                if buttons & mask != 0 {
                    events.push(InputKind::ButtonPressed { button: idx as u16 });
                } else {
                    events.push(InputKind::ButtonReleased { button: idx as u16 });
                }
            }
        }

        // DPad → hat(0)
        let new_hat: i8 = Self::compute_hat(buttons);
        if new_hat != self.last_hat {
            self.last_hat = new_hat;
            events.push(InputKind::HatChanged {
                hat: 0,
                value: new_hat as i16,
            });
        }

        self.last_buttons = buttons;
        events
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> &str {
        #[cfg(debug_assertions)]
        {
            debug_assert_eq!(self.fingerprint.to_string(), self.fingerprint_str);
        }
        &self.fingerprint_str
    }

    fn metadata(&self) -> DeviceMeta {
        self.meta.clone()
    }

    fn describe(&self) -> Vec<ChannelDesc> {
        // XInput does not expose HID descriptors, so we publish a stable,
        // conventional channel map that matches `poll()`:
        // - 6 axes (0..5): LX, LY, RX, RY, LT, RT
        // - 10 buttons (0..9): A, B, X, Y, LB, RB, Back, Start, LThumb, RThumb
        // - 1 hat (0): DPad (-1|0..7)
        //
        // All axes are normalized to [-1.0, 1.0] in `poll()`, so we describe
        // logical_min/max as [-1, 1] here.
        let mut out = Vec::new();

        // Axes
        const AXIS_NAMES: [&str; MAX_AXES] = ["LX", "LY", "RX", "RY", "LT", "RT"];
        for (i, &name) in AXIS_NAMES.iter().enumerate() {
            out.push(ChannelDesc {
                kind: ChannelKind::Axis,
                idx: i as u16,
                name: Some(name.to_string()),
                logical_min: -1,
                logical_max: 1,
                usage_page: None,
                usage: None,
            });
        }

        // Buttons (indices must match BUTTON_MAP in `poll()`)
        const BUTTON_NAMES: [&str; 10] = [
            "A", "B", "X", "Y", "LB", "RB", "Back", "Start", "LThumb", "RThumb",
        ];
        for (i, &name) in BUTTON_NAMES.iter().enumerate() {
            out.push(ChannelDesc {
                kind: ChannelKind::Button,
                idx: i as u16,
                name: Some(name.to_string()),
                logical_min: 0,
                logical_max: 1,
                usage_page: None,
                usage: None,
            });
        }

        // DPad -> Hat(0)
        out.push(ChannelDesc {
            kind: ChannelKind::Hat,
            idx: 0,
            name: Some("DPad".to_string()),
            logical_min: -1,
            logical_max: 7,
            usage_page: None,
            usage: None,
        });

        out
    }
}
