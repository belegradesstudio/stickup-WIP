use stickup::DeviceManager;

fn main() {
    let mut manager = DeviceManager::new();
    let snapshot = manager.snapshot();

    println!("--- Device Snapshot ---");
    for (device_id, state) in snapshot.iter() {
        println!("Device: {}", device_id);

        for (axis, value) in state.axes.iter() {
            println!("  Axis {} = {}", axis, value);
        }

        for (button, pressed) in state.buttons.iter() {
            println!(
                "  Button {} is {}",
                button,
                if *pressed { "pressed" } else { "released" }
            );
        }
    }
}
