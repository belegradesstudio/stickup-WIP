# Changelog

All notable changes to **StickUp** will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/).

---

## [0.2.0] – 2025-05-16

### ✨ Added

- **DeviceManager** struct: central manager for all input devices.
- Automatic discovery of HID and virtual devices based on features.
- `poll_all()` for raw event stream collection.
- `snapshot()` to build a per-frame snapshot of all axis/button states.
- Binding resolution helpers:
  - `get_axis("joy0.throttle")` → `Option<f32>`
  - `is_pressed("joy1.button5")` → `bool`

### 🧰 Improved

- Better device abstraction (via trait object).
- Cleaner internal structure for adding custom or virtual devices.
- Graceful handling of HID API init failure.

### 🔄 Changed

- Modularized manager logic into `manager.rs`
- Publicly re-exported `DeviceManager` as part of crate API

---

## [0.1.6] – 2025-05-14

- Initial public release on crates.io
- Basic HID device discovery and polling
- Virtual input backend with demo support
- Exposed `Device`, `InputEvent`, and device scan utilities
