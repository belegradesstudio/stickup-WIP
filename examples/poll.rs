use hidapi::HidApi;
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use stickup::backends::windows::probe_devices;
use stickup::event::ChannelKind;
use stickup::InputKind::{AxisMoved, ButtonPressed, ButtonReleased, HatChanged};

fn main() {
    let api = HidApi::new().expect("init hidapi");
    let mut devices = probe_devices(&api);

    println!("Discovered {} HID device(s)", devices.len());

    // Build label maps once from descriptors
    let mut axis_labels: HashMap<(usize, u16), String> = HashMap::new();
    let mut hat_labels: HashMap<(usize, u16), String> = HashMap::new();

    for (i, d) in devices.iter().enumerate() {
        println!("== {} ({}) ==", d.name(), d.id());
        for ch in d.describe() {
            println!(
                "  {:?} #{:02} name={:?} logical=[{}..{}] up={:?} u={:?}",
                ch.kind, ch.idx, ch.name, ch.logical_min, ch.logical_max, ch.usage_page, ch.usage
            );
            match ch.kind {
                ChannelKind::Axis => {
                    let label = ch.name.clone().unwrap_or_else(|| format!("A{}", ch.idx));
                    axis_labels.insert((i, ch.idx), label);
                }
                ChannelKind::Hat => {
                    let label = ch.name.clone().unwrap_or_else(|| format!("H{}", ch.idx));
                    hat_labels.insert((i, ch.idx), label);
                }
                _ => {}
            }
        }
    }

    // Aggregation buffers (per flush window)
    let flush_every = Duration::from_millis(40);
    let mut last_flush = Instant::now();
    let mut axes: HashMap<(usize, u16), f32> = HashMap::new();
    let mut hats: HashMap<(usize, u16), i16> = HashMap::new();
    let mut pressed: BTreeSet<(usize, u16)> = BTreeSet::new();
    let mut released: BTreeSet<(usize, u16)> = BTreeSet::new();

    loop {
        // 1) Poll all devices and aggregate changes
        for (i, d) in devices.iter_mut().enumerate() {
            let evs = d.poll();
            if evs.is_empty() {
                continue;
            }
            for ev in evs {
                match ev {
                    AxisMoved { axis, value } => {
                        axes.insert((i, axis), value);
                    }
                    HatChanged { hat, value } => {
                        hats.insert((i, hat), value);
                    }
                    ButtonPressed { button } => {
                        pressed.insert((i, button));
                        released.remove(&(i, button));
                    }
                    ButtonReleased { button } => {
                        released.insert((i, button));
                        pressed.remove(&(i, button));
                    }
                }
            }
        }

        // 2) Periodically flush a compact summary per device
        if last_flush.elapsed() >= flush_every {
            // Determine which device indices have any changes
            let mut changed: BTreeSet<usize> = BTreeSet::new();
            for &(di, _) in axes.keys() {
                changed.insert(di);
            }
            for &(di, _) in hats.keys() {
                changed.insert(di);
            }
            for &(di, _) in pressed.iter() {
                changed.insert(di);
            }
            for &(di, _) in released.iter() {
                changed.insert(di);
            }

            for di in changed {
                let dev = &devices[di];

                // Collect & format axes for this device (sorted by axis idx)
                let mut axis_items: Vec<(u16, f32)> = axes
                    .iter()
                    .filter_map(|(&(kdi, a), &v)| if kdi == di { Some((a, v)) } else { None })
                    .collect();
                axis_items.sort_by_key(|(a, _)| *a);

                let axes_str = if axis_items.is_empty() {
                    String::new()
                } else {
                    let mut parts = Vec::with_capacity(axis_items.len());
                    for (a, v) in axis_items {
                        let label = axis_labels
                            .get(&(di, a))
                            .cloned()
                            .unwrap_or_else(|| format!("A{}", a));
                        parts.push(format!("{}={:.3}", label, v));
                    }
                    parts.join(" ")
                };

                // Collect & format hats for this device
                let mut hat_items: Vec<(u16, i16)> = hats
                    .iter()
                    .filter_map(|(&(kdi, h), &v)| if kdi == di { Some((h, v)) } else { None })
                    .collect();
                hat_items.sort_by_key(|(h, _)| *h);

                let hats_str = if hat_items.is_empty() {
                    String::new()
                } else {
                    let mut parts = Vec::with_capacity(hat_items.len());
                    for (h, v) in hat_items {
                        let label = hat_labels
                            .get(&(di, h))
                            .cloned()
                            .unwrap_or_else(|| format!("H{}", h));
                        let val = if v < 0 {
                            "-".to_string()
                        } else {
                            v.to_string()
                        };
                        parts.push(format!("{}={}", label, val));
                    }
                    parts.join(" ")
                };

                // Collect pressed/released for this device
                let plus: Vec<u16> = pressed
                    .iter()
                    .filter_map(|&(kdi, b)| if kdi == di { Some(b) } else { None })
                    .collect();
                let minus: Vec<u16> = released
                    .iter()
                    .filter_map(|&(kdi, b)| if kdi == di { Some(b) } else { None })
                    .collect();

                let mut changes = Vec::new();
                for b in plus {
                    changes.push(format!("+{}", b));
                }
                for b in minus {
                    changes.push(format!("-{}", b));
                }
                let buttons_str = if changes.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", changes.join(","))
                };

                // Build the final line only with non-empty sections
                let mut sections = Vec::new();
                if !axes_str.is_empty() {
                    sections.push(axes_str);
                }
                if !hats_str.is_empty() {
                    sections.push(hats_str);
                }
                if !buttons_str.is_empty() {
                    sections.push(buttons_str);
                }

                if !sections.is_empty() {
                    println!("{}: {}", dev.id(), sections.join(" "));
                }
            }

            // Clear buffers after flush window
            axes.clear();
            hats.clear();
            pressed.clear();
            released.clear();
            last_flush = Instant::now();
        }

        // Keep CPU usage sane
        std::thread::sleep(Duration::from_millis(5));
    }
}
