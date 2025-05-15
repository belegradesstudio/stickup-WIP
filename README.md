# StickUp

A modular, high-performance input abstraction layer for Rust applications.
Built to handle physical and virtual devices with precision, simplicity, and flexibility.

Created by **Belegrade Studio**.

---

## Features

- 🔌 Plug-and-play device management (HID + virtual devices)
- 🎮 Axis + button input abstraction via `Device` trait
- 📋 Snapshot state or stream real-time events
- 🔧 Configurable `BindingProfile` system to map inputs to actions
- ⚙️ Optional feature flags (`hid`, `virtual`) to control backends
- 💡 Clean architecture, built for game engines, overlays, and tools

---

## Installation

Add this to your `Cargo.toml`:

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

## Examples

Run these with `cargo run --example`:

- `poll`: Print snapshot of all current input device states
- `virtual_demo`: Feed manual input into a virtual device

---

## Optional Features

- `hid` (default): Enables HID device support via `hidapi`
- `virtual`: Enables simulated virtual devices

---

## License

Licensed under the **Pact of the Amaranth Rite**. See `LICENSE` for details.
Inspired by the spirit of the MIT license, with deeper roots.

This crate uses hidapi, licensed under MIT or Apache-2.0.

---

## Philosophy

StickUp isn’t just about input. It’s about clarity, intention, and *presence*.
Part of the **CelerisTech** suite.

---

For questions, contributions, or commercial use:
**<belegrade@belegrades.gg>**
