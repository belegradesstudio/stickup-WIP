# StickUp

**StickUp** is a modular, high-performance input abstraction layer for Rust applications.  
It handles physical and virtual devices with precision, persistence, and simplicity.

Created by **Belegrade Studio**. Part of the **CelerisTech** suite.

---

## Features

- 🔌 Plug-and-play device management (`hidapi` + virtual devices)
- 🎮 Clean `Device` trait: axis + button abstraction
- 🧠 Persistent device identity — robust rebinding & hotplugging support
- 📋 Snapshot state or stream real-time `InputEvent`s
- 🔧 Flexible `BindingProfile` system to map inputs to actions
- ⚙️ Feature flags (`hid`, `virtual`) to tailor backend support
- 💡 Minimal dependencies. Built for tools, overlays, engines, and more.

---

## Installation

```toml
stickup = { version = "0.1.0", features = ["hid", "virtual"] }
```

---

## Quick Start

```rust
use stickup::DeviceManager;

fn main() {
    let mut manager = DeviceManager::new();
    let snapshot = manager.snapshot();

    for (id, state) in snapshot.iter() {
        println!("Device: {}", id);
        for (axis, value) in &state.axes {
            println!("  Axis {} = {}", axis, value);
        }
        for (button, pressed) in &state.buttons {
            println!("  Button {} is {}", button, if *pressed { "pressed" } else { "released" });
        }
    }
}
```

---

## Device Identity

StickUp assigns a stable fingerprint to each device based on its hardware signature:

```text
vendor_id:product_id:serial_number
# Example: 044f:0402:ABCD1234
```

If the device provides a serial number, this ID is persistent across USB ports, reboots, and sessions — making it perfect for rebindings, multi-device setups, and simulators.

---

## Examples

Run these with `cargo run --example`:

- `poll`: Print a snapshot of all connected device states
- `virtual_demo`: Feed manual input into a simulated device

---

## Optional Features

- `hid` (enabled by default): Enables HID device support
- `virtual`: Enables simulated virtual input devices

---

## License

Licensed under the **Pact of the Amaranth Rite**. See `LICENSE` for terms.  
Inspired by the MIT license, with deeper philosophical roots.

This crate uses `hidapi`, licensed under MIT or Apache-2.0.

---

## Philosophy

StickUp isn’t just about input. It’s about clarity, intentional systems, and persistent presence.  
Built for tools that know what they're listening to.

---

**Questions or contributions?**  
Reach out at **<belegrade@belegrades.gg>**
