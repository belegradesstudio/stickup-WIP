//! src/devices/bindings.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Axis or button control type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlType {
    Axis,
    Button,
}

/// Identifies an axis or button on a device.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ControlPath {
    pub control_id: String,
    pub control_type: ControlType,
}

/// Maps a control path to a named action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub device_id: String,
    pub control: ControlPath,
    pub action_name: String,
    pub invert: bool,
    pub deadzone: f32,
}

/// Serializable profile of input bindings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingProfile {
    pub name: String,
    pub description: Option<String>,
    pub bindings: Vec<Binding>,
}

/// Normalized output generated from device input and bindings.
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct BindingOutput {
    pub axis: HashMap<String, f32>,
    pub buttons: HashMap<String, bool>,
}

/// Snapshot of current axis/button states per device.
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct DeviceState {
    pub axes: HashMap<String, f32>,
    pub buttons: HashMap<String, bool>,
}

impl BindingProfile {
    /// Resolves bound actions from device states.
    pub fn resolve(&self, devices: &HashMap<String, DeviceState>) -> BindingOutput {
        let mut output = BindingOutput::default();

        for binding in &self.bindings {
            if let Some(state) = devices.get(&binding.device_id) {
                match binding.control.control_type {
                    ControlType::Axis => {
                        let mut value = state.get_axis(&binding.control.control_id);
                        if binding.invert {
                            value *= -1.0;
                        }
                        if value.abs() < binding.deadzone {
                            value = 0.0;
                        }
                        output.axis.insert(binding.action_name.clone(), value);
                    }
                    ControlType::Button => {
                        let pressed = state.get_button(&binding.control.control_id);
                        output.buttons.insert(binding.action_name.clone(), pressed);
                    }
                }
            }
        }

        output
    }
}

impl DeviceState {
    /// Gets the value of a named axis (0.0 if missing).
    pub fn get_axis(&self, name: &str) -> f32 {
        self.axes.get(name).copied().unwrap_or(0.0)
    }

    /// Gets the state of a named button (false if missing).
    pub fn get_button(&self, name: &str) -> bool {
        self.buttons.get(name).copied().unwrap_or(false)
    }
}
