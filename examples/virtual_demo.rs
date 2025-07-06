use stickup::backends::virtual_input::VirtualDevice;
use stickup::{Device, InputKind};

fn main() {
    // Create a virtual device with a custom ID and name
    let mut device = VirtualDevice::new("virtual:demo", "Demo Virtual Device");

    // Inject some sample input
    device.set_axis(0, 0.75);
    device.press_button(1);

    // Poll the device and print the emitted input events
    for event in device.poll() {
        match event {
            InputKind::AxisMoved { axis, value } => {
                println!("(Virtual) Axis {} = {}", axis, value);
            }
            InputKind::ButtonPressed { button } => {
                println!("(Virtual) Button {} pressed", button);
            }
            InputKind::ButtonReleased { button } => {
                println!("(Virtual) Button {} released", button);
            }
        }
    }
}
