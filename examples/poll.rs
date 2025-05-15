use stickup::DeviceManager;

fn main() {
    // Create a new manager and automatically detect devices
    let mut manager = DeviceManager::new();

    // Get a snapshot of all device states at this moment
    let snapshot = manager.snapshot();

    println!("--- Device Snapshot ---");

    // Iterate through each detected device
    for (device_id, state) in snapshot.iter() {
        println!("Device: {}", device_id);

        // Print all axis values
        for (axis, value) in &state.axes {
            println!("  Axis {} = {}", axis, value);
        }

        // Print all button states
        for (button, pressed) in &state.buttons {
            println!(
                "  Button {} is {}",
                button,
                if *pressed { "pressed" } else { "released" }
            );
        }
    }
}
