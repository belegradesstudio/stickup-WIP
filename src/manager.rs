//! High-level discovery and polling API.
//!
//! `Manager` hides platform details and offers a simple way to
//!  - discover devices
//!  - poll events across all devices
//!  - maintain a per-device [`DeviceState`] snapshot for bindings
//!
//! ```no_run
//! use stickup::{Manager, prelude::*};
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
use crate::binding::DeviceState;
use crate::device::Device;
use crate::event::{ChannelDesc, ChannelKind, InputKind};
use crate::metadata::DeviceMeta;
use crate::Result;
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
pub struct Manager {
    devices: Vec<Box<dyn Device>>,
    /// Stable `(axis/button/hat index -> label)` per device.
    labels: HashMap<String, LabelMaps>,
    /// Last-known per-device state.
    states: HashMap<String, DeviceState>,
    infos: Vec<ManagedInfo>,
    /// Cached backend descriptors per device (from `Device::describe()`).
    descs: HashMap<String, Vec<ChannelDesc>>,
}

impl Manager {
    /// Discover devices using enabled backends.
    /// Discover devices using enabled backends.
    pub fn discover() -> Result<Self> {
        let devices: Vec<Box<dyn Device>> = crate::backends::probe_devices(); // ⟵ remove `+ Send` for single thread use
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
        })
    }

    /// Construct from already created devices.
    pub fn from_devices(devices: Vec<Box<dyn Device>>) -> Self {
        // ⟵ remove `+ Send`
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
        }
    }

    /// Get backend-provided channel descriptors for a device.
    pub fn channels(&self, device_id: &str) -> Option<&[ChannelDesc]> {
        self.descs.get(device_id).map(|v| v.as_slice())
    }

    /// Poll all devices and yield `(device_id, event)` pairs.
    pub fn poll_events(&mut self) -> Vec<(String, InputKind)> {
        let mut out = Vec::new();
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
        out
    }

    /// Like [`poll_events`], but returns shared ids to avoid per-event `String` clone.
    /// This is additive and does not change existing APIs.
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
    pub fn snapshot(&self) -> crate::snapshot::Snapshot {
        // `Snapshot` is a tuple struct (see src/snapshot.rs)
        crate::snapshot::Snapshot(self.states.clone())
    }

    /// Snapshot current managed devices (id, name, meta).
    pub fn devices(&self) -> &[ManagedInfo] {
        &self.infos
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
