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
use std::collections::HashMap;

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

/// Cross-device manager.
pub struct Manager {
    devices: Vec<Box<dyn Device>>,
    /// Stable `(axis/button/hat index -> label)` per device.
    labels: HashMap<String, LabelMaps>,
    /// Last-known per-device state.
    states: HashMap<String, DeviceState>,
    infos: Vec<ManagedInfo>,
}

impl Manager {
    /// Discover devices using enabled backends.
    /// Discover devices using enabled backends.
    pub fn discover() -> Result<Self> {
        let devices: Vec<Box<dyn Device>> = crate::backends::probe_devices(); // ⟵ remove `+ Send`
        let mut labels: HashMap<String, LabelMaps> = HashMap::new();
        let mut states: HashMap<String, DeviceState> = HashMap::new();
        let mut infos: Vec<ManagedInfo> = Vec::new();

        for d in devices.iter() {
            let id = d.id().to_string();
            let name = d.name().to_string();
            let meta = d.metadata();
            let desc = d.describe();
            labels.insert(id.clone(), build_labels(&desc));
            states.insert(id.clone(), DeviceState::default());
            infos.push(ManagedInfo { id, name, meta });
        }

        Ok(Self {
            devices,
            labels,
            states,
            infos,
        })
    }

    /// Construct from already created devices.
    pub fn from_devices(devices: Vec<Box<dyn Device>>) -> Self {
        // ⟵ remove `+ Send`
        let mut labels: HashMap<String, LabelMaps> = HashMap::new();
        let mut states: HashMap<String, DeviceState> = HashMap::new();
        let mut infos: Vec<ManagedInfo> = Vec::new();
        for d in devices.iter() {
            let id = d.id().to_string();
            let name = d.name().to_string();
            let meta = d.metadata();
            let desc = d.describe();
            labels.insert(id.clone(), build_labels(&desc));
            states.insert(id.clone(), DeviceState::default());
            infos.push(ManagedInfo { id, name, meta });
        }
        Self {
            devices,
            labels,
            states,
            infos,
        }
    }

    /// Poll all devices and yield `(device_id, event)` pairs.
    pub fn poll_events(&mut self) -> Vec<(String, InputKind)> {
        let mut out = Vec::new();
        for i in 0..self.devices.len() {
            // Restrict the &mut borrow to this block
            let (id, events) = {
                let d = &mut self.devices[i];
                (d.id().to_string(), d.poll())
            };
            for ev in events {
                self.apply_event(&id, &ev);
                out.push((id.clone(), ev));
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
