//! Demonstrate `rescan()` and formatted device listing.
//!
//! Run: `cargo run --example hotplug`

use stickup::prelude::*;

fn main() -> stickup::Result<()> {
    let mut mgr = Manager::discover()?;

    println!("Devices:");
    for info in mgr.devices() {
        println!("- {}", info);
    }

    println!("\n-- Unplug or plug devices, then press Enter to rescan --");
    let _ = std::io::stdin().read_line(&mut String::new());

    let report = mgr.rescan();
    if report.added.is_empty() && report.removed.is_empty() {
        println!("No changes.");
    } else {
        for a in report.added {
            println!("[+] {}", a);
        }
        for r in report.removed {
            println!("[-] {}", r);
        }
    }

    println!("\nDevices:");
    for info in mgr.devices() {
        println!("- {}", info);
    }

    Ok(())
}
