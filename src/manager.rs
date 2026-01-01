//! High-level discovery and polling API.
//!
//! `Manager` hides platform details and offers a simple way to
//!  - discover devices
//!  - poll events across all devices
//!  - maintain a per-device [`DeviceState`] snapshot for bindings/UI
//!
//! ## Core model
//! `Manager` owns live device handles and maintains **last-known state** per device.
//! Polling updates that state and yields any deltas as events.
//!
//! - [`poll_events`](Manager::poll_events) updates internal state and returns `(device_id, InputKind)` pairs.
//! - [`snapshot`](Manager::snapshot) returns an **owned clone** of the last-known state. It does **not** poll.
//!
//! This separation keeps polling explicit and makes it easy to drive UIs/bindings from a stable
//! snapshot each frame/tick.
//!
//! ## Threading
//! `Manager` is intended to live on **one thread**. If you need multi-threaded interaction,
//! route calls through a single “bridge” thread that owns the `Manager` and communicates via
//! message passing.
//!
//! ## Windows Raw Input (optional)
//! On Windows, the host app may forward keyboard/mouse Raw Input (WM_INPUT) into `Manager`.
//! Raw Input packets are parsed and queued as **injected events**, which are drained on the
//! next call to [`poll_events`](Manager::poll_events).
//!
//! Note: the `*_shared` and `*_timed` polling helpers currently **do not** drain injected events.
//! If you rely on WM_INPUT injection, use [`poll_events`](Manager::poll_events) (or add a drain step).
//!
//! ```no_run
//! use stickup::Manager;
//!
//! let mut mgr = Manager::discover().expect("discover devices");
//! loop {
//!     for (dev_id, ev) in mgr.poll_events() {
//!         println!("{dev_id}: {ev:?}");
//!     }
//!     let snapshot = mgr.snapshot();
//!     // ... feed snapshot into bindings, ui, etc.
//! }
//! ```
#[cfg(target_os = "windows")]
use crate::backends::windows::raw_input;
use crate::binding::DeviceState;
use crate::device::Device;
use crate::event::{ChannelDesc, ChannelKind, InputKind};
use crate::metadata::DeviceMeta;
use crate::Result;
#[cfg(target_os = "windows")]
use core::ffi::c_void;
use core::fmt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

type NameMap = HashMap<u16, String>;

#[derive(Default)]
struct LabelMaps {
    axes: NameMap,
    buttons: NameMap,
    hats: NameMap,
}

fn default_name(kind: ChannelKind, idx: u16) -> String {
    match kind {
        ChannelKind::Axis => format!("axis{idx}"),
        ChannelKind::Button => format!("btn{idx}"),
        ChannelKind::Hat => format!("hat{idx}"),
    }
}

fn build_labels(descs: &[ChannelDesc]) -> LabelMaps {
    let mut lm = LabelMaps::default();
    for d in descs {
        let name = d
            .name
            .clone()
            .unwrap_or_else(|| default_name(d.kind.clone(), d.idx));
        match d.kind {
            ChannelKind::Axis => {
                lm.axes.insert(d.idx, name);
            }
            ChannelKind::Button => {
                lm.buttons.insert(d.idx, name);
            }
            ChannelKind::Hat => {
                lm.hats.insert(d.idx, name);
            }
        }
    }
    lm
}

/// Minimal info about a managed device.
///
/// Intended for UIs/tooling (device picker lists, rescan reporting, etc.).
#[derive(Clone, Debug)]
pub struct ManagedInfo {
    pub id: String,
    pub name: String,
    pub meta: DeviceMeta,
}

impl fmt::Display for ManagedInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.name.trim();
        if name.is_empty() {
            write!(f, "({})", self.id)
        } else {
            write!(f, "{} ({})", name, self.id)
        }
    }
}

/// Cross-device manager.
///
/// Owns device handles and maintains per-device last-known state in [`DeviceState`].
/// Use [`poll_events`](Manager::poll_events) to refresh state and receive deltas.
/// Use [`snapshot`](Manager::snapshot) for an owned, cloneable view each frame/tick.
pub struct Manager {
    devices: Vec<Box<dyn Device>>,
    /// Stable `(axis/button/hat index -> label)` per device.
    labels: HashMap<String, LabelMaps>,
    /// Last-known per-device state.
    states: HashMap<String, DeviceState>,
    infos: Vec<ManagedInfo>,
    /// Cached backend descriptors per device (from `Device::describe()`).
    descs: HashMap<String, Vec<ChannelDesc>>,
    /// Host-injected events (e.g., WM_INPUT keyboard/mouse) drained on next `poll_events()`.
    injected: Vec<(String, InputKind)>,
}

impl Manager {
    /// Discover devices using enabled backends.
    ///
    /// This is the typical entry point for applications. It probes enabled backends,
    /// seeds neutral device state (so snapshots have stable keys immediately),
    /// and caches channel descriptors for UI/binding use.
    pub fn discover() -> Result<Self> {
        let devices: Vec<Box<dyn Device>> = crate::backends::probe_devices();
        let mut labels: HashMap<String, LabelMaps> = HashMap::new();
        let mut states: HashMap<String, DeviceState> = HashMap::new();
        let mut infos: Vec<ManagedInfo> = Vec::new();
        let mut descs: HashMap<String, Vec<ChannelDesc>> = HashMap::new();

        for d in devices.iter() {
            let id = d.id().to_string();
            let name = d.name().to_string();
            let meta = d.metadata();
            let desc = d.describe();
            let lm = build_labels(&desc);
            labels.insert(id.clone(), lm);
            // cache descriptors
            descs.insert(id.clone(), desc.clone());
            // seed neutral states so UI has values immediately
            let mut st = DeviceState::default();
            if let Some(lbl) = labels.get(&id) {
                seed_neutral(&mut st, lbl, &desc);
            }
            states.insert(id.clone(), st);
            infos.push(ManagedInfo { id, name, meta });
        }

        Ok(Self {
            devices,
            labels,
            states,
            infos,
            descs,
            injected: Vec::new(),
        })
    }

    /// Construct from already created devices.
    ///
    /// This is mainly useful for tests, custom backend composition, or embedding StickUp
    /// into a host that manages device creation separately.
    pub fn from_devices(devices: Vec<Box<dyn Device>>) -> Self {
        let mut labels: HashMap<String, LabelMaps> = HashMap::new();
        let mut states: HashMap<String, DeviceState> = HashMap::new();
        let mut infos: Vec<ManagedInfo> = Vec::new();
        let mut descs: HashMap<String, Vec<ChannelDesc>> = HashMap::new();
        for d in devices.iter() {
            let id = d.id().to_string();
            let name = d.name().to_string();
            let meta = d.metadata();
            let desc = d.describe();
            let lm = build_labels(&desc);
            labels.insert(id.clone(), lm);
            descs.insert(id.clone(), desc.clone());
            let mut st = DeviceState::default();
            if let Some(lbl) = labels.get(&id) {
                seed_neutral(&mut st, lbl, &desc);
            }
            states.insert(id.clone(), st);
            infos.push(ManagedInfo { id, name, meta });
        }
        Self {
            devices,
            labels,
            states,
            infos,
            descs,
            injected: Vec::new(),
        }
    }

    /// Get backend-provided channel descriptors for a device.
    ///
    /// Descriptors come from [`Device::describe`](crate::device::Device::describe) and are cached
    /// at discovery/rescan time. They describe the *shape* of the device (axis/button/hat indices,
    /// optional names, logical ranges, usages when available).
    pub fn channels(&self, device_id: &str) -> Option<&[ChannelDesc]> {
        self.descs.get(device_id).map(|v| v.as_slice())
    }

    /// Poll all devices and yield `(device_id, event)` pairs.
    ///
    /// This updates internal per-device [`DeviceState`] and returns per-change deltas.
    /// It also drains any host-injected events queued via the Windows Raw Input helpers.
    pub fn poll_events(&mut self) -> Vec<(String, InputKind)> {
        let mut out = Vec::new();
        // 1) Poll normal devices.
        for i in 0..self.devices.len() {
            let (id, events) = {
                let d = &mut self.devices[i];
                (d.id().to_string(), d.poll())
            };
            for ev in events.into_iter() {
                self.apply_event(&id, &ev);
                out.push((id.clone(), ev));
            }
        }

        // 2) Drain host-injected events (e.g., WM_INPUT keyboard).
        let injected = std::mem::take(&mut self.injected);
        if !injected.is_empty() {
            //eprintln!("[stickup] draining injected: {} event(s)", injected.len());
        }
        for (id, ev) in injected {
            self.apply_event(&id, &ev);
            out.push((id, ev));
        }
        out
    }

    /// Like [`poll_events`], but returns shared ids to avoid per-event `String` clone.
    /// This is additive and does not change existing APIs.
    ///
    /// Note: this currently polls *device backends only* and does **not** drain host-injected
    /// (WM_INPUT) events. If you use Raw Input injection, prefer [`poll_events`](Manager::poll_events).
    pub fn poll_events_shared(&mut self) -> Vec<(Arc<str>, InputKind)> {
        let mut out = Vec::new();
        for i in 0..self.devices.len() {
            let (id_string, events) = {
                let d = &mut self.devices[i];
                (d.id().to_string(), d.poll())
            };
            // Create a shared id once per device for this batch
            let id_shared: Arc<str> = Arc::from(id_string.as_str());
            for ev in events.into_iter() {
                self.apply_event(&id_string, &ev);
                out.push((id_shared.clone(), ev));
            }
        }
        out
    }

    /// Poll with timestamps.
    ///
    /// Produces the same device-poll deltas as [`poll_events`](Manager::poll_events),
    /// but wraps each event with a capture timestamp.
    ///
    /// Note: this currently does **not** include drained injected (WM_INPUT) events.
    pub fn poll_events_timed(&mut self) -> Vec<(String, crate::event::InputEvent)> {
        let mut out = Vec::new();

        for i in 0..self.devices.len() {
            let (id, events) = {
                let d = &mut self.devices[i];
                (d.id().to_string(), d.poll())
            };
            let now = Instant::now();
            for ev in events.into_iter() {
                self.apply_event(&id, &ev);
                out.push((id.clone(), crate::event::InputEvent { at: now, kind: ev }));
            }
        }

        out
    }

    /// Timestamped polling with shared ids (no per-event `String` clone).
    ///
    /// Note: this currently does **not** include drained injected (WM_INPUT) events.
    pub fn poll_events_timed_shared(&mut self) -> Vec<(Arc<str>, crate::event::InputEvent)> {
        let mut out = Vec::new();
        for i in 0..self.devices.len() {
            let (id_string, events) = {
                let d = &mut self.devices[i];
                (d.id().to_string(), d.poll())
            };
            let id_shared: Arc<str> = Arc::from(id_string.as_str());
            let now = Instant::now();
            for ev in events.into_iter() {
                self.apply_event(&id_string, &ev);
                out.push((
                    id_shared.clone(),
                    crate::event::InputEvent { at: now, kind: ev },
                ));
            }
        }
        out
    }

    fn apply_event(&mut self, id: &str, ev: &InputKind) {
        let st = self.states.entry(id.to_string()).or_default();
        let Some(lbl) = self.labels.get(id) else {
            return;
        };
        match *ev {
            InputKind::AxisMoved { axis, value } => {
                let k = lbl
                    .axes
                    .get(&axis)
                    .cloned()
                    .unwrap_or_else(|| default_name(ChannelKind::Axis, axis));
                st.axes.insert(k, value);
            }
            InputKind::ButtonPressed { button } => {
                let k = lbl
                    .buttons
                    .get(&button)
                    .cloned()
                    .unwrap_or_else(|| default_name(ChannelKind::Button, button));
                st.buttons.insert(k, true);
            }
            InputKind::ButtonReleased { button } => {
                let k = lbl
                    .buttons
                    .get(&button)
                    .cloned()
                    .unwrap_or_else(|| default_name(ChannelKind::Button, button));
                st.buttons.insert(k, false);
            }
            InputKind::HatChanged { hat, value } => {
                let k = lbl
                    .hats
                    .get(&hat)
                    .cloned()
                    .unwrap_or_else(|| default_name(ChannelKind::Hat, hat));
                st.hats.insert(k, value);
            }
        }
    }

    /// Rescan devices and report adds/removes. Preserves existing state where possible.
    ///
    /// Devices are matched by `device_id` (backend-provided stable id). For devices that still
    /// exist after rescan, prior [`DeviceState`] is preserved and re-seeded to ensure stable keys.
    pub fn rescan(&mut self) -> RescanReport {
        let old_ids: HashSet<_> = self.infos.iter().map(|i| i.id.clone()).collect();
        let old_states = self.states.clone();

        let new_devs = crate::backends::probe_devices();
        let mut new_labels: HashMap<String, LabelMaps> = HashMap::new();
        let mut new_states: HashMap<String, DeviceState> = HashMap::new();
        let mut new_infos: Vec<ManagedInfo> = Vec::new();
        let mut new_descs: HashMap<String, Vec<ChannelDesc>> = HashMap::new();

        for d in new_devs.iter() {
            let id = d.id().to_string();
            let name = d.name().to_string();
            let meta = d.metadata();
            let desc = d.describe();
            let lm = build_labels(&desc);
            new_labels.insert(id.clone(), lm);
            new_descs.insert(id.clone(), desc.clone());
            let mut st = old_states.get(&id).cloned().unwrap_or_default();
            if let Some(lbl) = new_labels.get(&id) {
                seed_neutral(&mut st, lbl, &desc);
            }
            new_states.insert(id.clone(), st);
            new_infos.push(ManagedInfo { id, name, meta });
        }

        let new_ids: HashSet<_> = new_infos.iter().map(|i| i.id.clone()).collect();
        let removed: Vec<_> = old_ids.difference(&new_ids).cloned().collect();
        let added_idset: HashSet<_> = new_ids.difference(&old_ids).cloned().collect();
        let added: Vec<_> = new_infos
            .iter()
            .filter(|i| added_idset.contains(&i.id))
            .cloned()
            .collect();

        self.devices = new_devs;
        self.labels = new_labels;
        self.states = new_states;
        self.infos = new_infos;
        self.descs = new_descs;

        RescanReport { added, removed }
    }

    /// Get an immutable cloneable per-frame snapshot.
    ///
    /// This returns an **owned clone** of the last-known device state map.
    /// It does **not** poll devices. Call [`poll_events`](Manager::poll_events) first
    /// if you want to refresh state before snapshotting.
    pub fn snapshot(&self) -> crate::snapshot::Snapshot {
        // `Snapshot` is a tuple struct (see src/snapshot.rs)
        crate::snapshot::Snapshot(self.states.clone())
    }

    /// Snapshot current managed devices (id, name, meta).
    ///
    /// Useful for UI lists and device pickers.
    pub fn devices(&self) -> &[ManagedInfo] {
        &self.infos
    }

    // ==========================
    // Windows: WM_INPUT injection
    // ==========================
    //
    // The host app (AxisMirror, a game, etc.) owns the HWND/message loop and forwards
    // WM_INPUT lparam here. StickUp parses RAWINPUT and queues normalized events.

    /// Windows-only: parse a `WM_INPUT` lparam and enqueue any resulting events.
    ///
    /// The host is responsible for registering Raw Input devices on its HWND
    /// (RegisterRawInputDevices). This just parses and normalizes, and queues the
    /// resulting events to be drained on the next [`poll_events`](Manager::poll_events).
    #[cfg(target_os = "windows")]
    pub fn handle_wm_input(&mut self, lparam: isize) {
        let Some(pkt) = raw_input::read_wm_input(lparam) else {
            return;
        };
        self.handle_raw_input_packet(pkt);
    }
    /// Windows-only: parse a copied `RID_INPUT` payload (bytes returned by `GetRawInputData`)
    /// and enqueue any resulting events.
    ///
    /// This is the safe "deferred processing" path: the host must copy the WM_INPUT payload
    /// during the window proc, then it can hand the bytes to StickUp later (outside the proc).
    #[cfg(target_os = "windows")]
    pub fn handle_raw_input_bytes(&mut self, bytes: &[u8]) {
        let Some(pkt) = raw_input::read_raw_input_bytes(bytes) else {
            return;
        };
        self.handle_raw_input_packet(pkt);
    }

    #[cfg(target_os = "windows")]
    fn handle_raw_input_packet(&mut self, pkt: raw_input::RawInputPacket) {
        match pkt {
            raw_input::RawInputPacket::Keyboard(k) => {
                //eprintln!(
                //    "[stickup] rawkbd packet: scancode=0x{:02x} ext={} break={} hdev={:p}",
                //    k.scancode, k.is_extended, k.is_break, k.hdevice as *const c_void
                //);
                let key_idx = raw_input::pack_key_index(k.scancode, k.is_extended);

                let dev_id = match raw_input::device_name(k.hdevice) {
                    Some(s) => s,
                    None => format!("rawkbd:{:p}", k.hdevice as *const c_void),
                };

                self.ensure_keyboard_key_registered(&dev_id, key_idx);

                let ev = if k.is_break {
                    InputKind::ButtonReleased { button: key_idx }
                } else {
                    InputKind::ButtonPressed { button: key_idx }
                };
                //eprintln!("[stickup] inject => dev_id={} ev={:?}", dev_id, ev);
                self.injected.push((dev_id, ev));
            }

            raw_input::RawInputPacket::Mouse(m) => {
                // Button flag bits from RAWMOUSE usButtonFlags (RI_MOUSE_*).
                const RI_MOUSE_LEFT_BUTTON_DOWN: u16 = 0x0001;
                const RI_MOUSE_LEFT_BUTTON_UP: u16 = 0x0002;
                const RI_MOUSE_RIGHT_BUTTON_DOWN: u16 = 0x0004;
                const RI_MOUSE_RIGHT_BUTTON_UP: u16 = 0x0008;
                const RI_MOUSE_MIDDLE_BUTTON_DOWN: u16 = 0x0010;
                const RI_MOUSE_MIDDLE_BUTTON_UP: u16 = 0x0020;
                const RI_MOUSE_BUTTON_4_DOWN: u16 = 0x0040;
                const RI_MOUSE_BUTTON_4_UP: u16 = 0x0080;
                const RI_MOUSE_BUTTON_5_DOWN: u16 = 0x0100;
                const RI_MOUSE_BUTTON_5_UP: u16 = 0x0200;

                // Use the device interface path when available for "one device per physical mouse".
                let dev_id = match raw_input::device_name(m.hdevice) {
                    Some(s) => s,
                    None => format!("rawmouse:{:p}", m.hdevice as *const c_void),
                };

                self.ensure_mouse_registered(&dev_id);

                // Axes: 0=dx, 1=dy, 2=wheel, 3=hwheel (raw counts / wheel ticks).
                if m.dx != 0 {
                    self.injected.push((
                        dev_id.clone(),
                        InputKind::AxisMoved {
                            axis: 0,
                            value: m.dx as f32,
                        },
                    ));
                }
                if m.dy != 0 {
                    self.injected.push((
                        dev_id.clone(),
                        InputKind::AxisMoved {
                            axis: 1,
                            value: m.dy as f32,
                        },
                    ));
                }
                if m.wheel_delta != 0 {
                    // Standard WHEEL_DELTA is 120; exposing "ticks" is usually nicer.
                    self.injected.push((
                        dev_id.clone(),
                        InputKind::AxisMoved {
                            axis: 2,
                            value: (m.wheel_delta as f32) / 120.0,
                        },
                    ));
                }
                if m.hwheel_delta != 0 {
                    self.injected.push((
                        dev_id.clone(),
                        InputKind::AxisMoved {
                            axis: 3,
                            value: (m.hwheel_delta as f32) / 120.0,
                        },
                    ));
                }

                // Buttons: 0=L, 1=R, 2=M, 3=X1, 4=X2
                let f = m.buttons_flags;
                if (f & RI_MOUSE_LEFT_BUTTON_DOWN) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonPressed { button: 0 }));
                }
                if (f & RI_MOUSE_LEFT_BUTTON_UP) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonReleased { button: 0 }));
                }
                if (f & RI_MOUSE_RIGHT_BUTTON_DOWN) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonPressed { button: 1 }));
                }
                if (f & RI_MOUSE_RIGHT_BUTTON_UP) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonReleased { button: 1 }));
                }
                if (f & RI_MOUSE_MIDDLE_BUTTON_DOWN) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonPressed { button: 2 }));
                }
                if (f & RI_MOUSE_MIDDLE_BUTTON_UP) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonReleased { button: 2 }));
                }
                if (f & RI_MOUSE_BUTTON_4_DOWN) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonPressed { button: 3 }));
                }
                if (f & RI_MOUSE_BUTTON_4_UP) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonReleased { button: 3 }));
                }
                if (f & RI_MOUSE_BUTTON_5_DOWN) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonPressed { button: 4 }));
                }
                if (f & RI_MOUSE_BUTTON_5_UP) != 0 {
                    self.injected
                        .push((dev_id.clone(), InputKind::ButtonReleased { button: 4 }));
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    fn ensure_keyboard_key_registered(&mut self, dev_id: &str, key_idx: u16) {
        // Ensure device entry exists.
        if !self.labels.contains_key(dev_id) {
            // Create empty label maps.
            self.labels.insert(dev_id.to_string(), LabelMaps::default());
            // Create empty descriptor list.
            self.descs.insert(dev_id.to_string(), Vec::new());
            // Seed empty state.
            self.states
                .insert(dev_id.to_string(), DeviceState::default());
            // Add to device list for UIs.
            let mut meta = DeviceMeta::default();
            meta.bus = Some("rawinput".into());
            meta.path = Some(dev_id.to_string());
            self.infos.push(ManagedInfo {
                id: dev_id.to_string(),
                name: "Keyboard".into(),
                meta,
            });
        }

        // Ensure this specific key has a label + descriptor (so DeviceState updates work).
        let key_name = format!("key_{:04x}", key_idx);

        if let Some(lbl) = self.labels.get_mut(dev_id) {
            lbl.buttons
                .entry(key_idx)
                .or_insert_with(|| key_name.clone());
        }

        if let Some(descs) = self.descs.get_mut(dev_id) {
            let exists = descs
                .iter()
                .any(|d| d.kind == ChannelKind::Button && d.idx == key_idx);
            if !exists {
                descs.push(ChannelDesc {
                    kind: ChannelKind::Button,
                    idx: key_idx,
                    name: Some(key_name),
                    logical_min: 0,
                    logical_max: 1,
                    usage_page: None,
                    usage: None,
                });
            }
        }

        // Ensure state has an entry for this control so consumers see a stable key set.
        if let Some(st) = self.states.get_mut(dev_id) {
            st.buttons
                .entry(format!("key_{:04x}", key_idx))
                .or_insert(false);
        }
    }

    #[cfg(target_os = "windows")]
    fn ensure_mouse_registered(&mut self, dev_id: &str) {
        if !self.labels.contains_key(dev_id) {
            self.labels.insert(dev_id.to_string(), LabelMaps::default());
            self.descs.insert(dev_id.to_string(), Vec::new());
            self.states
                .insert(dev_id.to_string(), DeviceState::default());

            let mut meta = DeviceMeta::default();
            meta.bus = Some("rawinput".into());
            meta.path = Some(dev_id.to_string());
            self.infos.push(ManagedInfo {
                id: dev_id.to_string(),
                name: "Mouse".into(),
                meta,
            });
        }

        // Give the mouse a stable “shape” for bindings/UIs immediately.
        // Axes: 0=dx, 1=dy, 2=wheel, 3=hwheel
        // Buttons: 0=L, 1=R, 2=M, 3=X1, 4=X2
        let axes = [
            (0u16, "dx"),
            (1u16, "dy"),
            (2u16, "wheel"),
            (3u16, "hwheel"),
        ];
        let buttons = [
            (0u16, "lmb"),
            (1u16, "rmb"),
            (2u16, "mmb"),
            (3u16, "x1"),
            (4u16, "x2"),
        ];

        if let Some(lbl) = self.labels.get_mut(dev_id) {
            for (idx, name) in axes {
                lbl.axes.entry(idx).or_insert_with(|| name.to_string());
            }
            for (idx, name) in buttons {
                lbl.buttons.entry(idx).or_insert_with(|| name.to_string());
            }
        }

        if let Some(descs) = self.descs.get_mut(dev_id) {
            for (idx, name) in axes {
                let exists = descs
                    .iter()
                    .any(|d| d.kind == ChannelKind::Axis && d.idx == idx);
                if !exists {
                    descs.push(ChannelDesc {
                        kind: ChannelKind::Axis,
                        idx,
                        name: Some(name.to_string()),
                        logical_min: -32768,
                        logical_max: 32767,
                        usage_page: None,
                        usage: None,
                    });
                }
            }
            for (idx, name) in buttons {
                let exists = descs
                    .iter()
                    .any(|d| d.kind == ChannelKind::Button && d.idx == idx);
                if !exists {
                    descs.push(ChannelDesc {
                        kind: ChannelKind::Button,
                        idx,
                        name: Some(name.to_string()),
                        logical_min: 0,
                        logical_max: 1,
                        usage_page: None,
                        usage: None,
                    });
                }
            }
        }

        if let Some(st) = self.states.get_mut(dev_id) {
            // Seed axes/buttons so snapshot consumers see stable keys.
            for (_, name) in axes {
                st.axes.entry(name.to_string()).or_insert(0.0);
            }
            for (_, name) in buttons {
                st.buttons.entry(name.to_string()).or_insert(false);
            }
        }
    }
}

/// Added/removed devices after a rescan.
#[derive(Clone, Debug)]
pub struct RescanReport {
    pub added: Vec<ManagedInfo>,
    pub removed: Vec<String>,
}

// ------ helpers ------
fn seed_neutral(state: &mut DeviceState, labels: &LabelMaps, descs: &[ChannelDesc]) {
    for d in descs {
        let label = match d.kind {
            ChannelKind::Axis => labels
                .axes
                .get(&d.idx)
                .cloned()
                .unwrap_or_else(|| default_name(ChannelKind::Axis, d.idx)),
            ChannelKind::Button => labels
                .buttons
                .get(&d.idx)
                .cloned()
                .unwrap_or_else(|| default_name(ChannelKind::Button, d.idx)),
            ChannelKind::Hat => labels
                .hats
                .get(&d.idx)
                .cloned()
                .unwrap_or_else(|| default_name(ChannelKind::Hat, d.idx)),
        };
        match d.kind {
            ChannelKind::Axis => {
                state.axes.entry(label).or_insert(0.0);
            }
            ChannelKind::Button => {
                state.buttons.entry(label).or_insert(false);
            }
            ChannelKind::Hat => {
                state.hats.entry(label).or_insert(-1);
            }
        }
    }
}
