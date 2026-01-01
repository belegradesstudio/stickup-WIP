# StickUp 0.3.0

A modular, high-performance input device manager for Rust — built to *read what your devices are actually doing*.

StickUp focuses on **input ingestion** (reading devices). It is **not** a virtual device generator (it does not create vJoy/uinput-style devices).

[![Crates.io](https://img.shields.io/crates/v/stickup)](https://crates.io/crates/stickup)
[![Downloads](https://img.shields.io/crates/d/stickup)](https://crates.io/crates/stickup)
[![Join the Discord](https://img.shields.io/discord/1068768849186840738?label=chat&logo=discord)](https://discord.gg/EKeBNYnaSh)
[![Buy Me a Coffee](https://img.shields.io/badge/Buy%20me%20a%20coffee-Ko--fi-FF5E5B?logo=kofi&logoColor=white)](https://ko-fi.com/belegrades)
[![Follow on X](https://img.shields.io/badge/follow-%40BelegradeGG-1DA1F2?style=flat&logo=x)](https://x.com/BelegradeOfRuin)

---

## 🎮 What is StickUp?

**StickUp** is a modular input framework for Rust that provides:

- 🔌 **Device discovery** and hotplug-friendly rescans
- 🔁 **Polling** that yields deltas (`(device_id, InputKind)`)
- 🧊 **Snapshots** (owned, per-tick state maps) for UI + bindings
- 🧠 **Persistent identity** (fingerprinting) to keep bindings stable across reconnects
- 🧰 **Binding profiles** (deadzone/curve/gain/clamp + axis/button/2D mappings)
- 🪟 **Windows Raw Input injection** for keyboard/mouse (optional)

> Part of the **CelerisTech** stack by **Belegrade Studio**

---

## 🔐 Security Note

The name *stickup* was previously used in 2023 for a malicious crate that has since been removed from crates.io.

This version — authored by **Belegrade Studio** — is a clean and fully rewritten project, unrelated to the original.

✅ No `build.rs`  
✅ No network activity  
✅ 100% open and auditable

Transparency matters. Feel free to inspect the source or reach out directly.

---

## ✅ Platform support

- **Windows**: HID devices (via `hidapi` + HIDP parsing) and **XInput** controllers.
- Other platforms: backend support is currently limited / not implemented in this build.

---

## 📦 Installation

```toml
stickup = "0.3.0"
```

---

## 🚀 Quick start

Poll devices, print events, and take a per-tick snapshot:

```rust
use stickup::Manager;

fn main() {
    let mut mgr = Manager::discover().expect("discover devices");

    loop {
        // 1) Poll: updates internal state + yields deltas.
        for (id, ev) in mgr.poll_events() {
            println!("{id}: {ev:?}");
        }

        // 2) Snapshot: owned clone of last-known state (does NOT poll).
        let snap = mgr.snapshot();

        for (dev_id, state) in snap.iter() {
            let x = state.get_axis("X");
            let y = state.get_axis("Y");
            let trigger = state.get_button("Trigger");
            println!("{dev_id}: X={x:.2} Y={y:.2} trigger={trigger}");
        }
    }
}
```

---

## 🧬 Device identity

StickUp attempts to produce stable IDs using a fingerprint (VID/PID + serial when available).  
When serial isn’t available, it may fall back to a normalized device path segment.

Example ID shape:

```
044f:0402:ABCD1234
044f:0402@HID#VID_044F&PID_0402...
```

This keeps bindings stable across reconnects when the underlying platform exposes stable identity.

---

## 🧰 Bindings (optional)

StickUp includes a small binding schema to map device state into normalized actions:

- `Axis1d` → scalar actions
- `Axis2d` → vector actions (with optional radial deadzone)
- `Button` → boolean actions (or threshold an axis into a button)

> Recommendation: apply “feel” transforms (deadzone/curve/gain) in **one place**.  
> Double-processing transforms (backend + UI) can cause early saturation and reduced travel.

---

## 🪟 Windows Raw Input (optional)

If your host app owns a Win32 window proc, you can forward keyboard/mouse WM_INPUT into StickUp:

- `Manager::handle_wm_input(lparam)`
- `Manager::handle_raw_input_bytes(bytes)`

These events are queued as injected events and drained on the next `mgr.poll_events()` call.

Note: mouse deltas/wheel are currently reported as raw units (counts/ticks).  
A future release may introduce dedicated mouse event variants for clearer semantics.

---

## 📜 License

This project is licensed under the **Pact of the Amaranth Rite**.  
See [`LICENSE`](./LICENSE) for details.

---

## 💬 Connect

- ✉️ Email: belegrades.studio@gmail.com
- 💬 Discord: https://discord.gg/EKeBNYnaSh
- 🎮 Sim pilots, streamers/gamers & devs: I’d love to hear how you’re using StickUp.
