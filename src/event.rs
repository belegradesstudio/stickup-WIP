#[derive(Debug, Clone)]
pub enum InputEvent {
    AxisMoved { axis: u8, value: f32 },
    ButtonPressed { button: u8 },
    ButtonReleased { button: u8 },
}
