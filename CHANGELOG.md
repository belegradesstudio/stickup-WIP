# Changelog

All notable changes to **StickUp** will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/).

## [0.3.0] - 2025-10-30
### Added
- **Windows HIDP descriptor parser**: precise, per-usage decoding for axes, buttons, and hats (POV). Axes normalized to `[-1,1]`. Hats normalized to slots (`-1` neutral, `0..7`).
- **Hotplug**: `DeviceManager::rescan()` diff-adds physical devices and removes missing ones, preserving virtual devices. Cleans `last_state` for removed IDs.
- **Axis binding helper**: `AxisMotionThreshold` listener (app-layer policy) triggers when an axis moves by a given Δ from its baseline — ideal for “move ≥25% to bind” workflows.

### Improved
- **Uniqueness without serial**: device fingerprint now falls back to HID **path** when no serial number is present (`VID:PID@<path-segment>`). Ready to prefer container ID in a future update.

### Notes
- No background threads; no breaking behavior for existing `poll()` / `snapshot()` users.
- Public API remains lean; additions are opt-in helpers.


## [0.2.9] - 2025-07-05
### Added
- Full event system with `InputEventBus` supporting listener registration, filtering, and dispatch.
- Support for axis and button events, with custom filtering via `EventFilter` and `FilteredListener`.
- Built-in `Logger` for debugging input streams.
- Integration with `DeviceManager` for automatic event emission on polling and snapshot.

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
