fn main() {
    let mut mgr = stickup::DeviceManager::new();
    loop {
        let _events = mgr.poll_all();
        let snap = mgr.snapshot();
        for (dev_id, dev) in snap.iter() {
            print!("dev={} axes={}", dev_id, dev.axes.len());
            for (k, v) in dev.axes.iter() {
                print!(" {}={:.3}", k, v);
            }
            println!();
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
