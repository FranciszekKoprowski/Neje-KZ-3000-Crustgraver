/// Every packet starts with 0xFF and most end with 0x55.
/// This module builds raw byte packets — nothing is sent here,
/// it just returns Vec<u8> that serial.rs writes to the port.

pub const HEADER: u8 = 0xFF;
pub const FOOTER: u8 = 0x55;
pub const MAX_WIDTH: u16  = 490;
pub const MAX_HEIGHT: u16 = 490;

// ── small helper ────────────────────────────────────────────────────────────

/// Split a u16 into (high_byte, low_byte)
#[inline]
fn u16_bytes(v: u16) -> (u8, u8) {
    ((v >> 8) as u8, (v & 0xFF) as u8)
}

// ── init packets ─────────────────────────────────────────────────────────────

/// First handshake: FF 09 5A A5
pub fn init_ping() -> Vec<u8> {
    vec![HEADER, 0x09, 0x5A, 0xA5]
}

/// Second handshake: FF AA 08 01 01 5A A5 55
pub fn init_hello() -> Vec<u8> {
    vec![HEADER, 0xAA, 0x08, 0x01, 0x01, 0x5A, 0xA5, FOOTER]
}

// ── motion ───────────────────────────────────────────────────────────────────

/// Move laser head to absolute (x, y)
pub fn move_xy(x: u16, y: u16) -> Vec<u8> {
    let (xh, xl) = u16_bytes(x);
    let (yh, yl) = u16_bytes(y);
    vec![
        HEADER, 0xAA, 0x10, 0x05, 0x01, 0x50, 0x01,
        xh, xl, yh, yl,
        0x00, 0x00, 0x00, 0x00,
        FOOTER,
    ]
}

// ── preview window ───────────────────────────────────────────────────────────

/// Show bounding-box preview (laser traces the rectangle)
pub fn show_window(x: u16, y: u16, w: u16, h: u16) -> Vec<u8> {
    let (xh, xl) = u16_bytes(x);
    let (yh, yl) = u16_bytes(y);
    let (wh, wl) = u16_bytes(w);
    let (hh, hl) = u16_bytes(h);
    vec![
        HEADER, 0xAA, 0x10, 0x05, 0x01, 0x50, 0x02,
        xh, xl, yh, yl, wh, wl, hh, hl,
        FOOTER,
    ]
}

/// Stop the preview window
pub fn stop_window() -> Vec<u8> {
    vec![
        HEADER, 0xAA, 0x10, 0x05, 0x01, 0x50, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        FOOTER,
    ]
}

// ── laser power ───────────────────────────────────────────────────────────────

/// Set laser PWM power (0–255) and idle power
pub fn set_power(power: u8, idle: u8) -> Vec<u8> {
    vec![
        HEADER, 0xAA, 0x0B, 0x03, 0x01, 0x0F,
        power, idle,
        0x00, 0x00,
        FOOTER,
    ]
}

// ── carving control ───────────────────────────────────────────────────────────

pub fn stop_carving() -> Vec<u8> {
    vec![HEADER, 0xAA, 0x08, 0x02, 0x01, 0x01, 0x02, FOOTER]
}

pub fn pause_carving() -> Vec<u8> {
    vec![HEADER, 0xAA, 0x08, 0x02, 0x01, 0x01, 0x00, FOOTER]
}

// ── image header ──────────────────────────────────────────────────────────────

/// Sent before the raw pixel data.
/// `wr` = width rounded up to nearest multiple of 8.
/// `le` = total bytes of pixel data = (wr * h) / 8
pub fn image_info(x: u16, y: u16, w: u16, h: u16) -> Vec<u8> {
    let wr = (w + 7) & !7;          // round up to multiple of 8
    let le = (wr as u32 * h as u32) / 8;

    let (xh,  xl)  = u16_bytes(x);
    let (yh,  yl)  = u16_bytes(y);
    let (wrh, wrl) = u16_bytes(wr);
    let (hh,  hl)  = u16_bytes(h);
    let (leh, lel) = ((le >> 8) as u8, (le & 0xFF) as u8);
    let (wh,  wl)  = u16_bytes(w);

    vec![
        HEADER, 0xAA, 0x16, 0x04, 0x02, 0x01, 0x50,
        xh, xl, yh, yl,
        wrh, wrl, hh, hl,
        0x00, 0x00,
        leh, lel,
        wh, wl,
        FOOTER,
    ]
}

// ── response parsing ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum DeviceMessage {
    Online,
    ConnectionOk,
    UploadInfo(UploadEvent),
    Status { temperature: u8 },
    CarvingProgress { percent: u8, x: u16, y: u16 },
    AfterUpload { x: u16, y: u16, w: u16, h: u16 },
    Unknown(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum UploadEvent {
    ImageInfoReceived,
    WaitingForImage,
    UploadPercent(u8),
    UploadFinished,
}

/// Parse a packet that arrived from the device (already stripped of leading 0xFF).
/// The caller reads until 0x55 and passes the full frame including both delimiters.
pub fn parse_packet(data: &[u8]) -> Option<DeviceMessage> {
    if data.len() < 4 { return None; }

    // old-style 4-byte packet: FF D1 D2 D3
    if data[0] == HEADER && data[1] != 0xAA {
        return match data[1] {
            0x00 => Some(DeviceMessage::Online),
            0x02 => Some(DeviceMessage::ConnectionOk),
            _    => None,
        };
    }

    // new-style packet: FF AA <cmd> ...
    if data[0] != HEADER || data[1] != 0xAA { return None; }

    match data[2] {
        // upload info
        0x08 if data.len() >= 8 && data[3] == 0x04 && data[4] == 0x01 => {
            let ev = match data[5] {
                0x02 => UploadEvent::ImageInfoReceived,
                0x03 if data[6] == 0 => UploadEvent::WaitingForImage,
                0x03 => UploadEvent::UploadPercent(data[6]),
                0x04 => UploadEvent::UploadFinished,
                _ => return None,
            };
            Some(DeviceMessage::UploadInfo(ev))
        }

        // status (temperature)
        0x0B if data.len() >= 11 => Some(DeviceMessage::Status {
            temperature: data[5],
        }),

        // carving progress
        0x0E if data.len() >= 14 => Some(DeviceMessage::CarvingProgress {
            percent: data[6],
            x: (data[7] as u16) << 8 | data[8] as u16,
            y: (data[9] as u16) << 8 | data[10] as u16,
        }),

        // after-upload confirmation
        0x10 if data.len() >= 16 => Some(DeviceMessage::AfterUpload {
            x: (data[7] as u16) << 8 | data[8]  as u16,
            y: (data[9] as u16) << 8 | data[10] as u16,
            w: (data[11] as u16)<< 8 | data[12] as u16,
            h: (data[13] as u16)<< 8 | data[14] as u16,
        }),

        _ => Some(DeviceMessage::Unknown(data.to_vec())),
    }
}
