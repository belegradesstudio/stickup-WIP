//! src/devices/device.rs
/// Core trait for all input devices in StickUp.
///
/// Used for polling input and identifying devices across sessions.
///
/// Implementors must be **`Send`**. Use a single-threaded poller or your own
/// synchronization if you share devices across threads.
use crate::DeviceMeta;
pub trait Device: Send {
    /// Polls for new input events (e.g., axis motion, button press).
    fn poll(&mut self) -> Vec<crate::event::InputKind>;

    /// Returns a user-friendly display name (e.g., "T.16000M Joystick").
    fn name(&self) -> &str;

    /// Returns the device's persistent ID (`vendor:product[:serial]`).
    fn id(&self) -> &str;

    fn metadata(&self) -> DeviceMeta {
        DeviceMeta {
            bus: Some("unknown".into()),
            vid: None,
            pid: None,
            product_string: Some(self.name().to_string()),
            serial_number: None,
            usage_page: None,
            usage: None,
            interface_number: None,
            container_id: None,
            path: Some(self.id().to_string()),
        }
    }
}
