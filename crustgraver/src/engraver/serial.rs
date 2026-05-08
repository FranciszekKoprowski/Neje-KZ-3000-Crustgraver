use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use log::{debug, warn};
use parking_lot::Mutex;
use serialport::SerialPort;

use super::protocol::{self, DeviceMessage, FOOTER};

// ── public types ──────────────────────────────────────────────────────────────

pub type MessageCallback = Arc<dyn Fn(DeviceMessage) + Send + Sync + 'static>;

// ── SerialLayer ───────────────────────────────────────────────────────────────

/// Owns the serial port write-side and spins up a reader thread.
/// Cloneable handle: all clones share the same port via Arc<Mutex<>>.
#[derive(Clone)]
pub struct SerialLayer {
    port:    Arc<Mutex<Box<dyn SerialPort>>>,
    running: Arc<AtomicBool>,
}

impl SerialLayer {
    /// Open the port and start the background reader thread.
    /// `on_message` is called from the reader thread whenever a packet arrives.
    pub fn open(path: &str, on_message: MessageCallback) -> Result<Self> {
        let port = serialport::new(path, 57_600)
            .timeout(Duration::from_millis(100))
            .open()
            .with_context(|| format!("Failed to open serial port {path}"))?;

        let port = Arc::new(Mutex::new(port));
        let running = Arc::new(AtomicBool::new(true));

        // reader thread gets its own clone of the port arc
        let reader_port    = Arc::clone(&port);
        let reader_running = Arc::clone(&running);

        thread::spawn(move || {
            reader_loop(reader_port, reader_running, on_message);
        });

        Ok(Self { port, running })
    }

    /// Write raw bytes to the serial port.
    pub fn write(&self, data: &[u8]) -> Result<()> {
        let mut p = self.port.lock();
        p.write_all(data).context("Serial write failed")?;
        p.flush().context("Serial flush failed")?;
        debug!("TX {:02X?}", data);
        Ok(())
    }

    /// Gracefully stop the reader thread and close the port.
    pub fn close(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}


impl Drop for SerialLayer {
    fn drop(&mut self) {
        // only actually close when the last Arc is dropped
        if Arc::strong_count(&self.running) == 1 {
            self.close();
        }
    }
}

// ── reader loop ───────────────────────────────────────────────────────────────

/// Runs on a background thread. Reads bytes until 0x55, then parses.
fn reader_loop(
    port:     Arc<Mutex<Box<dyn SerialPort>>>,
    running:  Arc<AtomicBool>,
    callback: MessageCallback,
) {
    let mut buf = Vec::with_capacity(64);

    while running.load(Ordering::Relaxed) {
        let byte = {
            let mut p = port.lock();
            let mut b = [0u8; 1];
            match p.read(&mut b) {
                Ok(1)  => Some(b[0]),
                Ok(_)  => None,
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => None,
                Err(e) => {
                    warn!("Serial read error: {e}");
                    None
                }
            }
        };  // ← lock released here before we sleep or call the callback

        match byte {
            None => {
                thread::sleep(Duration::from_millis(5));
            }
            Some(FOOTER) => {
                // end of packet — try to parse whatever we collected
                if !buf.is_empty() {
                    debug!("RX {:02X?}", buf);
                    if let Some(msg) = protocol::parse_packet(&buf) {
                        callback(msg);
                    }
                    buf.clear();
                }
            }
            Some(b) => {
                buf.push(b);
            }
        }
    }
}


// ── port listing helper ───────────────────────────────────────────────────────

pub fn list_ports() -> Result<Vec<String>> {
    let ports = serialport::available_ports()
        .context("Could not enumerate serial ports")?;
    Ok(ports.into_iter().map(|p| p.port_name).collect())
}
