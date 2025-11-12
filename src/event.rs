#[derive(Clone, Debug)]
pub enum InputKind {
    AxisMoved { axis: u16, value: f32 }, // [-1.0, 1.0]
    ButtonPressed { button: u16 },
    ButtonReleased { button: u16 },
    HatChanged { hat: u16, value: i16 }, // -1 = neutral, 0..7 slots (Up=0 clockwise)
}

/// Timestamped input event captured by the Manager.
///
/// This is a lightweight wrapper over `InputKind` with a monotonic timestamp.
#[derive(Clone, Debug)]
pub struct InputEvent {
    pub at: std::time::Instant,
    pub kind: InputKind,
}

#[derive(Clone, Debug)]
pub enum ChannelKind {
    Axis,
    Button,
    Hat,
}

#[derive(Clone, Debug)]
pub struct ChannelDesc {
    pub kind: ChannelKind,
    pub idx: u16,
    pub name: Option<String>,
    pub logical_min: i32,
    pub logical_max: i32,
    pub usage_page: Option<u16>,
    pub usage: Option<u16>,
}
