/// A discrete input event from a device (polled or injected).
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// Axis moved to a new position.
    AxisMoved { axis: u8, value: f32 },

    /// Button was pressed.
    ButtonPressed { button: u8 },

    /// Button was released.
    ButtonReleased { button: u8 },
}
