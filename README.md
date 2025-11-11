
# StickUp

> 🚀 **Update: v0.2.9 is here!**  
> StickUp v0.2.9 adds:
- A full event system with `InputEventBus` supporting listener registration, filtering, and dispatch.
- Support for axis and button events, with custom filtering via `EventFilter` and `FilteredListener`.
- Built-in `Logger` for debugging input streams.
- Integration with `DeviceManager` for automatic event emission on polling and snapshot.

> Built to scale with sim rigs, overlays, game engines, and beyond.
- v0.3.0 coming soon with custom device input parsing.

📈 Huge thanks to everyone testing and sharing! Your support means the world to me. -Bel

[![Crates.io](https://img.shields.io/crates/v/stickup)](https://crates.io/crates/stickup)
[![Downloads](https://img.shields.io/crates/d/stickup)](https://crates.io/crates/stickup)
[![Join the Discord](https://img.shields.io/discord/1068768849186840738?label=chat&logo=discord)](https://discord.gg/EKeBNYnaSh)
[![Buy Me a Coffee](https://img.shields.io/badge/Buy%20me%20a%20coffee-Ko--fi-FF5E5B?logo=kofi&logoColor=white)](https://ko-fi.com/belegrades)
[![Follow on X](https://img.shields.io/badge/follow-%40BelegradeGG-1DA1F2?style=flat&logo=x)](https://x.com/BelegradeOfRuin)

---

## 🎮 What is StickUp?

**StickUp** is a modular, high-performance input framework for Rust.  
It supports both real HID devices and virtual inputs with clarity, precision, and stability.

> Part of the **CelerisTech** stack by **Belegrade Studio**

---

## 🔐 Security Note

The name *stickup* was previously used in 2023 for a malicious crate that has since been removed from crates.io.

This version — authored by **Belegrade Studio** — is a clean and fully rewritten project, unrelated to the original.

> ✅ No `build.rs`  
> ✅ No network activity  
> ✅ 100% open and auditable

Transparency matters. Feel free to inspect the source or reach out directly.

---

## ✨ Features

- 🔌 Plug-and-play device management via `DeviceManager`
- 🎮 Unified `Device` trait for axis + button input
- 🧠 Persistent device identity (hardware fingerprinting)
- 🧰 Binding resolution like `"joy0.axis1"` → `Option<f32>`
- 🔁 Snapshot-based polling and input state tracking
- 🔧 Hotplug-friendly and fully extendable
- 🛠 Supports `hid` and `virtual` backends via optional features
- 💡 Zero magic — minimal, intentional design

---

## 🧭 Philosophy

StickUp is about **presence, clarity, and persistence**.  
It doesn’t guess. It doesn’t simulate. It reflects exactly what your device is doing — no more, no less.

---

## 📦 Installation

```toml
stickup = { version = "0.2.1", features = ["hid", "virtual"] }
```

---

<details>
<summary>📦 Quick Start & Snapshot Example</summary>

```rust
use stickup::DeviceManager;

fn main() {
    let mut input = DeviceManager::new();
    input.snapshot(); // poll + build snapshot

    if let Some(throttle) = input.get_axis("joy0.throttle") {
        println!("Throttle: {:.2}", throttle);
    }

    if input.is_pressed("joy1.trigger") {
        println!("Trigger is pressed!");
    }

    // Full snapshot usage
    let state = input.snapshot();

    for (id, device_state) in state.iter() {
        println!("Device: {id}");

        for (axis, value) in &device_state.axes {
            println!("  Axis {axis}: {value:.2}");
        }

        for (button, pressed) in &device_state.buttons {
            println!("  Button {button}: {}", if *pressed { "Pressed" } else { "Released" });
        }
    }
}
```

</details>

---

## 🧬 Device Identity

StickUp assigns a stable, persistent ID to each device:

```
vendor_id:product_id:serial_number
# Example: 044f:0402:ABCD1234
```

This allows for consistent bindings across reboots and USB port changes.

---

## 🔍 Examples

Run any with:

```sh
cargo run --example <name>
```

- `poll` – Print a full snapshot of all input state
- `virtual_demo` – Feed input into a simulated virtual device

---

## 🛠️ Optional Features

| Feature | Description |
|--------|-------------|
| `hid` (default) | Enables HID device support via `hidapi` |
| `virtual` | Enables manually fed virtual devices |

---

## 🔮 Coming Next: Event Listeners

The next update will include a complete and functional input parser to ensure compatability with everything
from a simple arcade style joysticks to full HOSAS equipped sim-rigs.

## 📜 License

This project is licensed under the **Pact of the Amaranth Rite**.  
See [`LICENSE`](./LICENSE) for details.

### Third-Party Dependencies

- [`hidapi`](https://github.com/libusb/hidapi) — MIT/Apache-2.0 (HID support)
- [`serde`](https://github.com/serde-rs/serde) — MIT/Apache-2.0 (serialization)
- [`serde_json`](https://github.com/serde-rs/json) — MIT/Apache-2.0 (layout/config IO)
- [`toml`](https://github.com/alexcrichton/toml-rs) — MIT/Apache-2.0 (if config parsing used)

---

## 💬 Connect

- ✉️ Email: [belegrade@belegrades.gg](mailto:belegrade@belegrades.gg)
- 💬 Discord: [Join the Chat](https://discord.gg/EKeBNYnaSh)
- 🎮 Sim pilots & devs: I’d love to hear how you’re using StickUp.
