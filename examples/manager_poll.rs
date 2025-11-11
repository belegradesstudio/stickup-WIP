use stickup::Manager;

fn main() {
    let mut mgr = Manager::discover().expect("discover devices");
    println!("Devices:");
    for d in mgr.devices() {
        println!("- {} ({})", d.name, d.id);
    }
    loop {
        for (id, ev) in mgr.poll_events() {
            println!("{id}: {ev:?}");
        }
        // Sleep a touch to avoid pegging the CPU in the demo
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
