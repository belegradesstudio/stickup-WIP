//! src/devices/event.rs
/// A discrete input event from a device (polled or injected).
#[derive(Debug, Clone)]
pub struct InputEvent {
    pub device_id: String,
    pub kind: InputKind,
}

#[derive(Debug, Clone)]
pub enum InputKind {
    AxisMoved { axis: u8, value: f32 },
    ButtonPressed { button: u8 },
    ButtonReleased { button: u8 },
}
