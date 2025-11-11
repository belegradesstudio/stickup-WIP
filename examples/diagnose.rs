use hidapi::HidApi;

fn main() {
    let api = HidApi::new().expect("init hidapi");
    for info in api.device_list() {
        println!(
            "VID:PID={:04x}:{:04x} up=0x{:02x} u=0x{:02x} iface={} prod={:?} ser={:?} path={}",
            info.vendor_id(),
            info.product_id(),
            info.usage_page(),
            info.usage(),
            info.interface_number(),
            info.product_string(),
            info.serial_number(),
            info.path().to_string_lossy()
        );
    }
}
