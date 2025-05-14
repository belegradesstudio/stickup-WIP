use crate::{Device, InputEvent};
use hidapi::{DeviceInfo, HidApi, HidDevice};

pub struct HidInputDevice {
    id: String,
    name: String,
    raw: HidDevice,
}

impl HidInputDevice {
    pub fn new(info: &DeviceInfo, api: &HidApi) -> Option<Self> {
        if let Ok(device) = info.open_device(api) {
            Some(Self {
                id: format!("{}:{}", info.vendor_id(), info.product_id()),
                name: info.product_string().unwrap_or("Unknown").to_string(),
                raw: device,
            })
        } else {
            None
        }
    }
}

impl Device for HidInputDevice {
    fn poll(&mut self) -> Vec<InputEvent> {
        let mut buf = [0u8; 64]; // Adjust buffer size as needed
        let events = Vec::new();

        match self.raw.read_timeout(&mut buf, 1) {
            Ok(size) if size > 0 => {
                // Here you would parse the buffer into input events
                // For now, just debug print:
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
        &self.id
    }
}

pub fn probe_devices(api: &HidApi) -> Vec<Box<dyn Device>> {
    let mut found = Vec::new();
    for info in api.device_list() {
        if let Some(dev) = HidInputDevice::new(info, api) {
            found.push(Box::new(dev) as Box<dyn Device>);
        }
    }
    found
}
