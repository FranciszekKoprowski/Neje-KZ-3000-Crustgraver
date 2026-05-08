mod engraver;
use engraver::Engraver;

use anyhow::Result;
use std::{thread, time::Duration};

fn main() -> Result<()> {
    // RUST_LOG=info cargo run -- to see device messages
    env_logger::init();

    // ── list ports ────────────────────────────────────────────────────────────
    let ports = Engraver::list_ports()?;
    println!("Available ports: {ports:?}");

    let port = ports
        .iter()
        .find(|p| p.contains("USB") || p.contains("ACM"))
        .cloned()
        .unwrap_or_else(|| "/dev/ttyUSB0".to_string());

    println!("Using port: {port}");

    // ── connect ───────────────────────────────────────────────────────────────
    let mut e = Engraver::connect(&port)?;
    thread::sleep(Duration::from_secs(1));

    // ── set power (safe test value) ───────────────────────────────────────────
    e.set_power(80, 80)?;
    thread::sleep(Duration::from_millis(200));

    // ── preview test (traces a bounding box) ─────────────────────────────────
    println!("Showing preview window for 3 seconds...");
    e.show_preview(50, 50, 100, 100)?;
    thread::sleep(Duration::from_secs(3));
    e.stop_preview()?;
    thread::sleep(Duration::from_secs(1));

    // ── jog test ─────────────────────────────────────────────────────────────
    println!("Jogging...");
    e.move_right()?;
    thread::sleep(Duration::from_millis(300));
    e.move_down()?;
    thread::sleep(Duration::from_millis(300));
    e.move_left()?;
    thread::sleep(Duration::from_millis(300));
    e.move_up()?;
    thread::sleep(Duration::from_millis(300));

    // ── burn test (comment out if you don't want to actually engrave) ─────────
    println!("Burning logo.png ...");
    e.burn_image("logo.png", 50, 50)?;
    e.wait_for_completion(0);
    println!("Done!");

    println!("All tests passed.");
    Ok(())
}

