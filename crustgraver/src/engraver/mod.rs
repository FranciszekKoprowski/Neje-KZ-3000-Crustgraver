pub mod image;
pub mod protocol;
pub mod serial;

use std::{
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

use anyhow::Result;
use log::info;

use protocol::{DeviceMessage, MAX_HEIGHT, MAX_WIDTH};
use serial::{MessageCallback, SerialLayer};

// ── Engraver ──────────────────────────────────────────────────────────────────

/// High-level API, the only type needed in main.rs and the GUI.
pub struct Engraver {
    serial:   SerialLayer,
    /// Shared progress value updated by the reader thread (0-100).
    progress: Arc<Mutex<u8>>,
    /// Notified whenever progress changes so `wait_for_completion` can wake up.
    progress_cv: Arc<Condvar>,
    location: [u16; 2],
}

impl Engraver {
    /// Connect to the device on `port` (e.g. `"/dev/ttyUSB0"`).
    pub fn connect(port: &str) -> Result<Self> {
        let progress    = Arc::new(Mutex::new(100u8));
        let progress_cv = Arc::new(Condvar::new());

        let prog_cb = Arc::clone(&progress);
        let cv_cb   = Arc::clone(&progress_cv);

        let on_message: MessageCallback = Arc::new(move |msg| {
            match &msg {
                DeviceMessage::CarvingProgress { percent, x, y } => {
                    info!("Progress: {percent}%  pos=({x},{y})");
                    let mut p = prog_cb.lock().unwrap();
                    *p = *percent;
                    cv_cb.notify_all();
                }
                DeviceMessage::Online        => info!("Device online"),
                DeviceMessage::ConnectionOk  => info!("Connection OK"),
                DeviceMessage::Status { temperature } => {
                    info!("Temperature: {temperature}°C");
                }
                DeviceMessage::UploadInfo(ev) => info!("Upload: {ev:?}"),
                DeviceMessage::AfterUpload { x, y, w, h } => {
                    info!("Upload confirmed — area ({x},{y}) {w}×{h}");
                }
                DeviceMessage::Unknown(raw)  => info!("Unknown packet: {raw:02X?}"),
            }
        });

        let serial = SerialLayer::open(port, on_message)?;

        // handshake
        serial.write(&protocol::init_ping())?;
        thread::sleep(Duration::from_millis(200));
        serial.write(&protocol::init_hello())?;
        thread::sleep(Duration::from_millis(200));

        info!("Engraver connected on {port}");

        Ok(Self {
            serial,
            progress,
            progress_cv,
            location: [MAX_WIDTH / 2, MAX_HEIGHT / 2],
        })
    }

    // ── motion ────────────────────────────────────────────────────────────────

    pub fn move_to(&mut self, x: u16, y: u16) -> Result<()> {
        let x = x.min(MAX_WIDTH);
        let y = y.min(MAX_HEIGHT);
        self.location = [x, y];
        self.serial.write(&protocol::move_xy(x, y))
    }

    pub fn move_up(&mut self)    -> Result<()> {
        let [x, y] = self.location;
        self.move_to(x, y.saturating_sub(4))
    }
    pub fn move_down(&mut self)  -> Result<()> {
        let [x, y] = self.location;
        self.move_to(x, (y + 4).min(MAX_HEIGHT))
    }
    pub fn move_left(&mut self)  -> Result<()> {
        let [x, y] = self.location;
        self.move_to(x.saturating_sub(4), y)
    }
    pub fn move_right(&mut self) -> Result<()> {
        let [x, y] = self.location;
        self.move_to((x + 4).min(MAX_WIDTH), y)
    }

    // ── preview ───────────────────────────────────────────────────────────────

    pub fn show_preview(&self, x: u16, y: u16, w: u16, h: u16) -> Result<()> {
        self.serial.write(&protocol::show_window(x, y, w, h))
    }

    pub fn stop_preview(&self) -> Result<()> {
        self.serial.write(&protocol::stop_window())
    }

    // ── power ─────────────────────────────────────────────────────────────────

    /// Set laser power (0-255).  A safe starting value is around 70.
    pub fn set_power(&self, power: u8, idle: u8) -> Result<()> {
        self.serial.write(&protocol::set_power(power, idle))
    }

    // ── carving control ───────────────────────────────────────────────────────

    pub fn stop(&self)  -> Result<()> { self.serial.write(&protocol::stop_carving())  }
    pub fn pause(&self) -> Result<()> { self.serial.write(&protocol::pause_carving()) }

    // ── image burning ─────────────────────────────────────────────────────────

    /// Load, encode, and send an image.  Blocks until the device acknowledges
    /// the upload header; actual carving progress is reported via the callback.
    pub fn burn_image(&self, path: &str, x: u16, y: u16) -> Result<()> {
        let img = image::load_image(path)?;
        let w   = img.width() as u16;
        let h   = img.height() as u16;

        // send metadata header
        self.serial.write(&protocol::image_info(x, y, w, h))?;
        thread::sleep(Duration::from_secs(1)); // device needs time to prepare

        // encode and send pixel data
        let payload = image::encode_image(&img)?;
        self.serial.write(&payload)?;
        info!("Image sent ({} bytes)", payload.len());

        // reset progress so wait_for_completion works
        *self.progress.lock().unwrap() = 0;
        Ok(())
    }

    /// Block until carving reaches 100 % (or timeout ms, 0 = no timeout).
    pub fn wait_for_completion(&self, timeout_ms: u64) {
        let deadline = if timeout_ms > 0 {
            Some(std::time::Instant::now() + Duration::from_millis(timeout_ms))
        } else {
            None
        };

        let mut p = self.progress.lock().unwrap();
        loop {
            if *p >= 100 { break; }
            p = match deadline {
                Some(d) => {
                    let remaining = d.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() { break; }
                    self.progress_cv.wait_timeout(p, remaining).unwrap().0
                }
                None => self.progress_cv.wait(p).unwrap(),
            };
        }
    }

    pub fn progress(&self) -> u8 {
        *self.progress.lock().unwrap()
    }

    pub fn list_ports() -> Result<Vec<String>> {
        serial::list_ports()
    }
}

impl Drop for Engraver {
    fn drop(&mut self) {
        self.serial.close();
    }
}

