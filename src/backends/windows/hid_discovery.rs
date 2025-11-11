#![cfg(target_os = "windows")]

use crate::backends::windows::hid_device::HidInputDevice;
use crate::backends::windows::hidp_parser::WinHidpParser;
use crate::device::{Device, DeviceFingerprint};
use crate::metadata::DeviceMeta;
use hidapi::{DeviceInfo, HidApi};

fn accept_device(info: &DeviceInfo) -> bool {
    // Windows: Generic Desktop only
    let up = info.usage_page();
    let u = info.usage();
    if up != 0x01 {
        return false;
    } // Generic Desktop
      // Accept: Joystick(0x04), Gamepad(0x05), Multi-axis(0x08)
    let is_game = matches!(u, 0x04 | 0x05 | 0x08);
    // Reject Mouse(0x02), Keyboard(0x06)
    let is_mouse_or_kbd = matches!(u, 0x02 | 0x06);
    is_game && !is_mouse_or_kbd
}

fn fingerprint(info: &DeviceInfo) -> DeviceFingerprint {
    DeviceFingerprint {
        vendor_id: info.vendor_id(),
        product_id: info.product_id(),
        serial_number: info.serial_number().map(|s| s.to_owned()),
        path: Some(info.path().to_string_lossy().to_string()),
    }
}

fn meta(info: &DeviceInfo) -> DeviceMeta {
    let interface_number = {
        let n = info.interface_number();
        if n >= 0 {
            Some(n)
        } else {
            None
        }
    };
    Some(info.path().to_string_lossy().to_string());
    DeviceMeta {
        bus: Some("usb".into()),
        vid: Some(info.vendor_id()),
        pid: Some(info.product_id()),
        product_string: info.product_string().map(|s| s.to_string()),
        serial_number: info.serial_number().map(|s| s.to_string()),
        usage_page: Some(info.usage_page()),
        usage: Some(info.usage()),
        interface_number,
        container_id: None,
        path: Some(info.path().to_string_lossy().to_string()),
    }
}

pub fn probe_devices(api: &HidApi) -> Vec<Box<dyn Device>> {
    let mut out: Vec<Box<dyn Device>> = Vec::new();

    for info in api.device_list() {
        if !accept_device(info) {
            continue;
        }

        // HIDP parser is mandatory. If it fails, skip the device.
        if let Some(parser) = WinHidpParser::new(info) {
            if let Some(dev) = HidInputDevice::new(info, api, parser, fingerprint(info), meta(info))
            {
                out.push(Box::new(dev));
            }
        }
    }
    out
}
