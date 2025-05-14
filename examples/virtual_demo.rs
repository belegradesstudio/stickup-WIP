use stickup::backends::virtual_input::VirtualDevice;
use stickup::{Device, InputEvent};

fn main() {
    let mut device = VirtualDevice::new("virtual:demo", "Demo Virtual Device");

    // Feed some test input
    device.set_axis(0, 0.75);
    device.press_button(1);

    // Poll and print results
    for event in device.poll() {
        match event {
            InputEvent::AxisMoved { axis, value } => {
                println!("(Virtual) Axis {} = {}", axis, value);
            }
            InputEvent::ButtonPressed { button } => {
                println!("(Virtual) Button {} pressed", button);
            }
            InputEvent::ButtonReleased { button } => {
                println!("(Virtual) Button {} released", button);
            }
        }
    }
}
