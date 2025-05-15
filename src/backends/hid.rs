use crate::{Device, InputEvent};
use hidapi::{DeviceInfo, HidApi, HidDevice};

/// HID device implementation of the [`Device`] trait.
pub struct HidInputDevice {
    /// Structured device identity.
    #[allow(dead_code)]
    fingerprint: DeviceFingerprint,
    /// Cached ID string (vendor:product[:serial]).
    fingerprint_str: String,
    /// Human-readable name (from USB metadata).
    name: String,
    /// Raw HID device handle.
    raw: HidDevice,
}

impl HidInputDevice {
    /// Opens a HID device and constructs a `HidInputDevice`, if available.
    pub fn new(info: &DeviceInfo, api: &HidApi) -> Option<Self> {
        if let Ok(device) = info.open_device(api) {
            let fingerprint = DeviceFingerprint::from_info(info);
            let fingerprint_str = fingerprint.to_string();

            Some(Self {
                fingerprint,
                fingerprint_str,
                name: info.product_string().unwrap_or("Unknown").to_string(),
                raw: device,
            })
        } else {
            None
        }
    }
}

impl Device for HidInputDevice {
    /// Reads raw data from the device (currently debug only).
    fn poll(&mut self) -> Vec<InputEvent> {
        let mut buf = [0u8; 64];
        let events = Vec::new();

        match self.raw.read_timeout(&mut buf, 1) {
            Ok(size) if size > 0 => {
                println!("{} reported {} bytes: {:?}", self.name, size, &buf[..size]);
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("{} read error: {:?}", self.name, e);
            }
        }

        events
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> &str {
        &self.fingerprint_str
    }
}

/// Discovers available HID devices and wraps them as `Device`s.
pub fn probe_devices(api: &HidApi) -> Vec<Box<dyn Device>> {
    let mut found = Vec::new();
    for info in api.device_list() {
        if let Some(dev) = HidInputDevice::new(info, api) {
            found.push(Box::new(dev) as Box<dyn Device>);
        }
    }
    found
}

/// Unique identifier for a physical device.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceFingerprint {
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: Option<String>,
}

impl DeviceFingerprint {
    /// Builds a fingerprint from HID device metadata.
    pub fn from_info(info: &hidapi::DeviceInfo) -> Self {
        Self {
            vendor_id: info.vendor_id(),
            product_id: info.product_id(),
            serial_number: info.serial_number().map(|s| s.to_owned()),
        }
    }

    /// Formats the fingerprint as a persistent ID string.
    pub fn to_string(&self) -> String {
        match &self.serial_number {
            Some(serial) => format!("{:04x}:{:04x}:{}", self.vendor_id, self.product_id, serial),
            None => format!("{:04x}:{:04x}", self.vendor_id, self.product_id),
        }
    }
}
