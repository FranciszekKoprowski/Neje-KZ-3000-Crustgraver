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

// ── shared state (readable from the GUI) ─────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct DeviceState {
    pub progress: u8,            // 0-100
    pub temperature: Option<u8>, // °C, None until first status packet
    pub position: [u16; 2],
    pub burning: bool,
}

// ── Engraver ──────────────────────────────────────────────────────────────────

pub struct Engraver {
    serial: SerialLayer,
    state: Arc<Mutex<DeviceState>>,
    progress_cv: Arc<Condvar>,
}

impl Engraver {
    pub fn connect(port: &str) -> Result<Self> {
        let state = Arc::new(Mutex::new(DeviceState {
            progress: 100,
            ..Default::default()
        }));
        let progress_cv = Arc::new(Condvar::new());

        let state_cb = Arc::clone(&state);
        let cv_cb = Arc::clone(&progress_cv);

        let on_message: MessageCallback = Arc::new(move |msg| match &msg {
            DeviceMessage::CarvingProgress { percent, x, y } => {
                info!("Progress: {percent}%  pos=({x},{y})");
                let mut s = state_cb.lock().unwrap();
                s.progress = *percent;
                s.position = [*x, *y];
                if *percent >= 100 {
                    s.burning = false;
                }
                cv_cb.notify_all();
            }
            DeviceMessage::Status { temperature } => {
                info!("Temperature: {temperature}°C");
                state_cb.lock().unwrap().temperature = Some(*temperature);
            }
            DeviceMessage::Online => info!("Device online"),
            DeviceMessage::ConnectionOk => info!("Connection OK"),
            DeviceMessage::UploadInfo(ev) => info!("Upload: {ev:?}"),
            DeviceMessage::AfterUpload { x, y, w, h } => {
                info!("Upload confirmed — area ({x},{y}) {w}×{h}");
            }
            DeviceMessage::Unknown(raw) => info!("Unknown packet: {raw:02X?}"),
        });

        let serial = SerialLayer::open(port, on_message)?;

        serial.write(&protocol::init_ping())?;
        thread::sleep(Duration::from_millis(200));
        serial.write(&protocol::init_hello())?;
        thread::sleep(Duration::from_millis(200));

        info!("Engraver connected on {port}");

        Ok(Self {
            serial,
            state,
            progress_cv,
        })
    }

    // ── state snapshot (cheap clone for the GUI) ──────────────────────────────

    pub fn state(&self) -> DeviceState {
        self.state.lock().unwrap().clone()
    }

    // ── motion ────────────────────────────────────────────────────────────────

    pub fn move_to(&mut self, x: u16, y: u16) -> Result<()> {
        let x = x.min(MAX_WIDTH);
        let y = y.min(MAX_HEIGHT);
        self.state.lock().unwrap().position = [x, y];
        self.serial.write(&protocol::move_xy(x, y))
    }

    pub fn move_up(&mut self) -> Result<()> {
        let [x, y] = self.state().position;
        self.move_to(x, y.saturating_sub(4))
    }
    pub fn move_down(&mut self) -> Result<()> {
        let [x, y] = self.state().position;
        self.move_to(x, (y + 4).min(MAX_HEIGHT))
    }
    pub fn move_left(&mut self) -> Result<()> {
        let [x, y] = self.state().position;
        self.move_to(x.saturating_sub(4), y)
    }
    pub fn move_right(&mut self) -> Result<()> {
        let [x, y] = self.state().position;
        self.move_to((x + 4).min(MAX_WIDTH), y)
    }

    pub fn jog(&mut self, dx: i32, dy: i32) -> Result<()> {
        let [x, y] = self.state().position;
        let nx = (x as i32 + dx).clamp(0, MAX_WIDTH as i32) as u16;
        let ny = (y as i32 + dy).clamp(0, MAX_HEIGHT as i32) as u16;
        self.move_to(nx, ny)
    }

    // ── preview ───────────────────────────────────────────────────────────────

    pub fn show_preview(&self, x: u16, y: u16, w: u16, h: u16) -> Result<()> {
        self.serial.write(&protocol::show_window(x, y, w, h))
    }

    pub fn stop_preview(&self) -> Result<()> {
        self.serial.write(&protocol::stop_window())
    }

    // ── power ─────────────────────────────────────────────────────────────────

    pub fn set_power(&self, power: u8, idle: u8) -> Result<()> {
        self.serial.write(&protocol::set_power(power, idle))
    }

    // ── carving control ───────────────────────────────────────────────────────

    pub fn stop(&self) -> Result<()> {
        self.state.lock().unwrap().burning = false;
        self.serial.write(&protocol::stop_carving())
    }

    pub fn pause(&self) -> Result<()> {
        self.serial.write(&protocol::pause_carving())
    }

    // ── image burning ─────────────────────────────────────────────────────────

    /// Load, threshold, encode and send an image.
    /// `threshold` 0-255: pixels darker than this value will be burned.
    pub fn burn_image(
        &self,
        path: &str,
        x: u16,
        y: u16,
        threshold: u8,
        invert: bool,
    ) -> Result<()> {
        let raw_img = image::load_image(path)?;
        let thresholded = image::threshold_image(&raw_img, threshold, invert);
        let w = thresholded.width() as u16;
        let h = thresholded.height() as u16;

        self.serial.write(&protocol::image_info(x, y, w, h))?;
        thread::sleep(Duration::from_secs(1));

        let payload = image::encode_image(&thresholded)?;
        self.serial.write(&payload)?;
        info!("Image sent ({} bytes, {}×{} px)", payload.len(), w, h);

        {
            let mut s = self.state.lock().unwrap();
            s.progress = 0;
            s.burning = true;
        }
        Ok(())
    }

    /// Accepts a pre-loaded DynamicImage, thresholds it, and sends it to the device.
    pub fn burn_dynamic_image(
        &self,
        img: &::image::DynamicImage,
        x: u16,
        y: u16,
        threshold: u8,
        invert: bool,
    ) -> Result<()> {
        let thresholded = image::threshold_image(img, threshold, invert);
        let w = thresholded.width() as u16;
        let h = thresholded.height() as u16;

        self.serial.write(&protocol::image_info(x, y, w, h))?;
        thread::sleep(Duration::from_secs(1));

        let payload = image::encode_image(&thresholded)?;
        self.serial.write(&payload)?;
        info!("Image sent ({} bytes, {}×{} px)", payload.len(), w, h);

        {
            let mut s = self.state.lock().unwrap();
            s.progress = 0;
            s.burning = true;
        }
        Ok(())
    }

    // ── wait for completion ───────────────────────────────────────────────────

    pub fn wait_for_completion(&self, timeout_ms: u64) {
        let deadline = if timeout_ms > 0 {
            Some(std::time::Instant::now() + Duration::from_millis(timeout_ms))
        } else {
            None
        };

        let mut s = self.state.lock().unwrap();
        loop {
            if s.progress >= 100 {
                break;
            }
            s = match deadline {
                Some(d) => {
                    let rem = d.saturating_duration_since(std::time::Instant::now());
                    if rem.is_zero() {
                        break;
                    }
                    self.progress_cv.wait_timeout(s, rem).unwrap().0
                }
                None => self.progress_cv.wait(s).unwrap(),
            };
        }
    }

    pub fn progress(&self) -> u8 {
        self.state.lock().unwrap().progress
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
