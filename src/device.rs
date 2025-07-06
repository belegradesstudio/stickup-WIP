//! src/devices/device.rs
/// Core trait for all input devices in StickUp.
///
/// Used for polling input and identifying devices across sessions.
pub trait Device {
    /// Polls for new input events (e.g., axis motion, button press).
    fn poll(&mut self) -> Vec<crate::devices::event::InputKind>;

    /// Returns a user-friendly display name (e.g., "T.16000M Joystick").
    fn name(&self) -> &str;

    /// Returns the device's persistent ID (`vendor:product[:serial]`).
    fn id(&self) -> &str;
}
