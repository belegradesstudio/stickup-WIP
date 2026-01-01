//! Binding profiles and transforms.
//!
//! This module defines a small schema for turning per-device state into a normalized set
//! of actions (scalar axes, buttons, and 2D vectors).
//!
//! ## “Raw” vs “transformed”
//! StickUp backends are responsible for **decoding** device reports and producing coherent
//! values (e.g. axes normalized into `[-1, 1]`, buttons boolean, hats as `-1`/`0..7`).
//!
//! This module is for **policy**: deadzones, curves, gains, and mappings that are typically
//! owned by the application (UI/bindings/gameplay).
//!
//! Important: avoid applying shaping transforms in *multiple* layers (e.g. backend + UI).
//! If you run deadzone/curve/gain twice, you can get early saturation and reduced travel.
//!
//! # Overview
//! - [`DeviceState`]: snapshot of per-device inputs by **names** (`"X"`, `"Y"`, `"0"`, …).
//! - [`AxisTransform`]/[`AxisCurve`]: shaping, deadzone, invert, gain, clamp.
//! - [`BindingRule`]: declarative mapping (Axis1d / Button / Axis2d).
//! - [`BindingProfile`]: a named set of rules with `serde` serialization.
//! - [`BindingProfile::resolve`]: apply rules to device snapshots → [`BindingOutput`].
//!
//! # Conventions
//! - Axis values are assumed normalized to `[-1.0, 1.0]` **before** transforms.
//! - Button values are boolean; axes can be thresholded to act like buttons.
//! - Control IDs are free-form strings (e.g. `"X"`, `"RZ"`, `"3"`); choose a naming
//!   scheme that matches your device parser(s). A good default is to use `ChannelDesc.name`
//!   when available (or stable fallbacks like `"axis0"`, `"btn1"`, `"hat0"`).
//!
//! # Examples
//! Resolving a simple profile (two axes into a 2D action and a fire button):
//! ```ignore
//! use std::collections::HashMap;
//! use stickup::binding::*;
//!
//! // Device state snapshot
//! let mut dev = DeviceState::default();
//! dev.axes.insert("X".into(), 0.2);
//! dev.axes.insert("Y".into(), -0.4);
//! dev.buttons.insert("Trigger".into(), true);
//! dev.hats.insert("hat0".into(), 0); // Hats (optional): -1 = neutral, 0..7 = directions
//!
//! // Profile with two rules
//! let profile = BindingProfile {
//!     version: 1,
//!     name: "demo".into(),
//!     description: None,
//!     bindings: vec![
//!         BindingRule::Axis2d {
//!             device_id: "js0".into(),
//!             control: ControlPath2D {
//!                 x: ControlPath { control_id: "X".into(), control_type: ControlType::Axis },
//!                 y: ControlPath { control_id: "Y".into(), control_type: ControlType::Axis },
//!             },
//!             action: "move".into(),
//!             xform_x: AxisTransform::default(),
//!             xform_y: AxisTransform::default(),
//!             radial_deadzone: true,
//!             radial_deadzone_size: 0.1,
//!         },
//!         BindingRule::Button {
//!             device_id: "js0".into(),
//!             control: ControlPath { control_id: "Trigger".into(), control_type: ControlType::Button },
//!             action: "fire".into(),
//!             axis_press_threshold: None,
//!         },
//!     ],
//! };
//!
//! let mut devices = HashMap::new();
//! devices.insert("js0".into(), dev);
//!
//! let out = profile.resolve(&devices);
//! assert_eq!(out.buttons.get("fire").copied(), Some(true));
//! assert!(out.vec2.get("move").is_some());
//! ```
//!
//! ## API Notes
//! - **Serialization:** `BindingRule` uses `#[serde(tag = "kind", rename_all = "snake_case")]`.
//! - **Back-compat:** legacy [`Binding`] can be converted via [`Binding::to_rule`].
//! - **Defaults:** helper fns (`default_deadzone`, etc.) document implicit values.
//! - **Missing inputs:** missing devices/controls resolve as inactive (`0.0`/`false`/neutral).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/* =========================
   Core device state (runtime)
========================= */

/// Snapshot of current axis/button states for a single device.
///
/// Keys are **control IDs** (free-form strings like `"X"`, `"RZ"`, or `"2"`).
///
/// These IDs should be stable across runs if you intend to serialize profiles.
/// (For HID-backed devices, using descriptor-provided names or `"axis{idx}"`/`"btn{idx}"`
/// fallbacks keeps profiles predictable.)
#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct DeviceState {
    /// Named axis values (normalized `[-1.0, 1.0]` by convention).
    pub axes: HashMap<String, f32>,
    /// Named button states.
    pub buttons: HashMap<String, bool>,
    /// Named hat (POV/D-pad) states: `-1` for neutral, `0..7` for the eight directions.
    ///
    /// Conventionally labeled as `"hat0"`, `"hat1"`, etc. (the Windows HIDP parser follows this).
    #[serde(default)]
    pub hats: HashMap<String, i16>,
}

impl DeviceState {
    /// Get the value of a named axis (returns `0.0` if missing).
    #[inline]
    pub fn get_axis(&self, name: &str) -> f32 {
        self.axes.get(name).copied().unwrap_or(0.0)
    }

    /// Get the state of a named button (returns `false` if missing).
    #[inline]
    pub fn get_button(&self, name: &str) -> bool {
        self.buttons.get(name).copied().unwrap_or(false)
    }

    /// Get the value of a named hat (returns `-1`/neutral if missing).
    ///
    /// Values: `-1` = neutral, `0..7` = directions, starting at up and advancing clockwise.
    #[inline]
    pub fn get_hat(&self, name: &str) -> i16 {
        self.hats.get(name).copied().unwrap_or(-1)
    }
}

/* =========================
   Control identification
========================= */

/// Control categories addressable by a binding rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlType {
    /// Continuous input channel (normalized `[-1, 1]`).
    Axis,
    /// Binary input channel.
    Button,
}

/// Identifies a concrete control on a device.
///
/// Use simple, stable string IDs (e.g., `"X"`, `"Y"`, `"RZ"`, `"0"`, …).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ControlPath {
    /// Device-local identifier for the control.
    pub control_id: String,
    /// Control category.
    pub control_type: ControlType,
}

/* =========================
   Transform helpers
========================= */

fn default_deadzone() -> f32 {
    0.05
}
fn default_gain() -> f32 {
    1.0
}
fn default_min() -> f32 {
    -1.0
}
fn default_max() -> f32 {
    1.0
}

/// Response curve for axis shaping.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AxisCurve {
    /// `y = x` (linear).
    #[serde(rename = "linear")]
    Linear,
    /// `y = sign(x) * |x|^gamma` where `gamma > 0` (`<1` soft, `>1` stiff).
    #[serde(rename = "power")]
    Power {
        /// Exponent; also accepts `"p"` from older profiles.
        #[serde(alias = "p")]
        gamma: f32,
    },
}

impl Default for AxisCurve {
    fn default() -> Self {
        AxisCurve::Power { gamma: 1.0 } // effectively linear when gamma=1
    }
}

/// Per-axis transform pipeline.
///
/// Applies **deadzone with continuity** → **invert** → **curve** → **gain** → **clamp**.
///
/// Inputs are expected to already be normalized to `[-1, 1]` (or close to it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxisTransform {
    /// Multiply output by `-1` when `true`.
    #[serde(default)]
    pub invert: bool,

    /// Values with `|x| < deadzone` become `0`; others are rescaled to keep continuity.
    #[serde(default = "default_deadzone")]
    pub deadzone: f32,

    /// Post-curve scalar gain. (Alias: `"scale"` for older profiles.)
    #[serde(default = "default_gain", alias = "scale")]
    pub gain: f32,

    /// Curve shaping (linear by default; power curve carries its own exponent).
    #[serde(default)]
    pub curve: AxisCurve,

    /// Clamp final result into `[min, max]` (defaults to `[-1, 1]`).
    #[serde(default = "default_min")]
    pub min: f32,
    #[serde(default = "default_max")]
    pub max: f32,
}

impl Default for AxisTransform {
    fn default() -> Self {
        Self {
            invert: false,
            deadzone: default_deadzone(),
            gain: default_gain(),
            curve: AxisCurve::default(),
            min: default_min(),
            max: default_max(),
        }
    }
}

impl AxisTransform {
    /// Apply the transform pipeline to a normalized input `x ∈ [-1, 1]`.
    ///
    /// Steps: deadzone w/ continuity → invert → curve → gain → clamp.
    ///
    /// Note: if you already applied deadzone/curve/gain elsewhere, applying this again
    /// can cause early saturation and reduced travel.
    #[inline]
    pub fn apply(&self, x: f32) -> f32 {
        // 1) deadzone with continuity remap
        let dz = self.deadzone.clamp(0.0, 0.95);
        let mut v = {
            let s = x.signum();
            let a = x.abs();
            if a <= dz {
                0.0
            } else {
                // map [dz,1] → [0,1]
                s * ((a - dz) / (1.0 - dz))
            }
        };

        // 2) invert
        if self.invert {
            v = -v;
        }

        // 3) curve
        v = match self.curve {
            AxisCurve::Linear => v,
            AxisCurve::Power { gamma } => {
                let g = gamma.max(0.0001); // avoid 0^0/NaN
                v.signum() * v.abs().powf(g)
            }
        };

        // 4) gain
        v *= self.gain;

        // 5) clamp
        let lo = self.min.min(self.max);
        let hi = self.min.max(self.max);
        v.clamp(lo, hi)
    }
}

/* =========================
   Binding rules
========================= */

/// Two 1D control paths bundled as a 2D input (e.g., left stick).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlPath2D {
    /// X component source.
    pub x: ControlPath,
    /// Y component source.
    pub y: ControlPath,
}

/// Declarative binding rules.
///
/// Tagged enum (`kind: "axis1d" | "button" | "axis2d"`) to support clean `serde` IO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BindingRule {
    /// Map a single control to a scalar action.
    Axis1d {
        /// Device identifier (matches keys in the `devices` map passed to [`BindingProfile::resolve`]).
        device_id: String,
        /// Source control on that device.
        control: ControlPath,
        /// Destination action name (key under [`BindingOutput::axis`]).
        action: String,
        /// Per-axis transform (defaults provided).
        #[serde(default)]
        xform: AxisTransform,
    },
    /// Map a control to a boolean action (button or thresholded axis).
    Button {
        /// Device identifier.
        device_id: String,
        /// Source control path.
        control: ControlPath,
        /// Destination action name (key under [`BindingOutput::buttons`]).
        action: String,
        /// Optional: threshold for axis→button synthesis (absolute), default `0.5`.
        #[serde(default)]
        axis_press_threshold: Option<f32>,
        // Future: toggle/hold semantics can live here.
    },
    /// Map two controls into a 2D vector action with optional radial deadzone.
    Axis2d {
        /// Device identifier.
        device_id: String,
        /// Source x/y control paths.
        control: ControlPath2D,
        /// Destination action name (key under [`BindingOutput::vec2`]).
        action: String,
        /// X transform.
        #[serde(default)]
        xform_x: AxisTransform,
        /// Y transform.
        #[serde(default)]
        xform_y: AxisTransform,
        /// Apply radial deadzone instead of per-axis deadzones.
        #[serde(default)]
        radial_deadzone: bool,
        /// Radial deadzone radius when `radial_deadzone` is `true` (default `0.05`).
        #[serde(default = "default_deadzone")]
        radial_deadzone_size: f32,
    },
}

/* =========================
   Profiles & Outputs
========================= */

/// Serializable profile: a named collection of binding rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingProfile {
    /// Schema version for forward/backward migrations.
    #[serde(default)]
    pub version: u16,
    /// Human-readable profile name.
    pub name: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Rules that define how inputs map to actions.
    #[serde(default)]
    pub bindings: Vec<BindingRule>,
}

/// Normalized output produced by resolving a profile against device states.
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct BindingOutput {
    /// Scalar axis actions (e.g., `"rudder"`, `"throttle"`).
    #[serde(default)]
    pub axis: HashMap<String, f32>,
    /// Button actions (e.g., `"fire"`, `"gear_up"`).
    #[serde(default)]
    pub buttons: HashMap<String, bool>,
    /// 2D axis actions (e.g., `"stick"`, `"dual_axis"`).
    #[serde(default)]
    pub vec2: HashMap<String, [f32; 2]>,
}

impl BindingProfile {
    /// Resolve bound actions from current device states.
    ///
    /// Missing devices or controls are treated as inactive (`0.0`/`false`).
    ///
    /// This function is pure (no side effects): it only reads `devices` and produces output.
    #[inline]
    pub fn resolve(&self, devices: &HashMap<String, DeviceState>) -> BindingOutput {
        let mut out = BindingOutput::default();

        for rule in &self.bindings {
            match rule {
                BindingRule::Axis1d {
                    device_id,
                    control,
                    action,
                    xform,
                } => {
                    if let Some(st) = devices.get(device_id) {
                        let raw = match control.control_type {
                            ControlType::Axis => st.get_axis(&control.control_id),
                            ControlType::Button => {
                                if st.get_button(&control.control_id) {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                        };
                        let v = xform.apply(raw);
                        out.axis.insert(action.clone(), v);
                    }
                }

                BindingRule::Button {
                    device_id,
                    control,
                    action,
                    axis_press_threshold,
                } => {
                    if let Some(st) = devices.get(device_id) {
                        let pressed = match control.control_type {
                            ControlType::Button => st.get_button(&control.control_id),
                            ControlType::Axis => {
                                let thr = axis_press_threshold.unwrap_or(0.5).abs().min(0.99);
                                st.get_axis(&control.control_id).abs() >= thr
                            }
                        };
                        out.buttons.insert(action.clone(), pressed);
                    }
                }

                BindingRule::Axis2d {
                    device_id,
                    control,
                    action,
                    xform_x,
                    xform_y,
                    radial_deadzone,
                    radial_deadzone_size,
                } => {
                    if let Some(st) = devices.get(device_id) {
                        let rx = match control.x.control_type {
                            ControlType::Axis => st.get_axis(&control.x.control_id),
                            ControlType::Button => {
                                if st.get_button(&control.x.control_id) {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                        };
                        let ry = match control.y.control_type {
                            ControlType::Axis => st.get_axis(&control.y.control_id),
                            ControlType::Button => {
                                if st.get_button(&control.y.control_id) {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                        };

                        let mut x = xform_x.apply(rx);
                        let mut y = xform_y.apply(ry);

                        if *radial_deadzone {
                            let dz = radial_deadzone_size.abs().min(0.95);
                            let r = (x * x + y * y).sqrt();
                            if r <= dz {
                                x = 0.0;
                                y = 0.0;
                            } else {
                                // map (dz..1) → (0..1) preserving direction
                                let t = (r - dz) / (1.0 - dz);
                                if r > 0.0 {
                                    let k = t / r;
                                    x *= k;
                                    y *= k;
                                }
                            }
                        }

                        out.vec2.insert(action.clone(), [x, y]);
                    }
                }
            }
        }

        out
    }
}

/* =========================
   Back-compat shim (optional)
========================= */

/// Legacy single-axis binding kept for compatibility with older profiles.
///
/// Prefer using [`BindingRule`] going forward.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    /// Device identifier.
    pub device_id: String,
    /// Source control path.
    pub control: ControlPath,
    /// Destination action name.
    pub action_name: String,

    // Older fields; mapped into AxisTransform via `to_rule`.
    /// Legacy invert flag.
    #[serde(default)]
    pub invert: bool,
    /// Legacy deadzone.
    #[serde(default = "default_deadzone")]
    pub deadzone: f32,
    /// Legacy scale (maps to [`AxisTransform::gain`]).
    #[serde(default)]
    pub scale: Option<f32>,
}

impl Binding {
    /// Convert legacy single-axis binding into a modern [`BindingRule::Axis1d`].
    #[inline]
    pub fn to_rule(self) -> BindingRule {
        let mut xform = AxisTransform {
            invert: self.invert,
            deadzone: self.deadzone,
            ..AxisTransform::default()
        };
        if let Some(s) = self.scale {
            xform.gain = s;
        }
        BindingRule::Axis1d {
            device_id: self.device_id,
            control: self.control,
            action: self.action_name,
            xform,
        }
    }
}
