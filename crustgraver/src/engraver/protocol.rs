/// Every packet starts with 0xFF and most end with 0x55.
/// This module builds raw byte packets — nothing is sent here,
/// it just returns Vec<u8> that serial.rs writes to the port.
 
pub const HEADER: u8 = 0xFF;
pub const FOOTER: u8 = 0x55;
pub const MAX_WIDTH:  u16 = 490;
pub const MAX_HEIGHT: u16 = 490;
 
// ── small helper ─────────────────────────────────────────────────────────────
 
#[inline]
fn u16_bytes(v: u16) -> (u8, u8) {
    ((v >> 8) as u8, (v & 0xFF) as u8)
}
 
// ── init packets ─────────────────────────────────────────────────────────────
 
pub fn init_ping() -> Vec<u8> {
    vec![HEADER, 0x09, 0x5A, 0xA5]
}
 
pub fn init_hello() -> Vec<u8> {
    vec![HEADER, 0xAA, 0x08, 0x01, 0x01, 0x5A, 0xA5, FOOTER]
}
 
// ── motion ───────────────────────────────────────────────────────────────────
 
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
 
pub fn stop_window() -> Vec<u8> {
    vec![
        HEADER, 0xAA, 0x10, 0x05, 0x01, 0x50, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        FOOTER,
    ]
}
 
// ── laser power ───────────────────────────────────────────────────────────────
 
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
 
pub fn image_info(x: u16, y: u16, w: u16, h: u16) -> Vec<u8> {
    let wr = (w + 7) & !7;
    let le = (wr as u32 * h as u32) / 8;
 
    let (xh,  xl)  = u16_bytes(x);
    let (yh,  yl)  = u16_bytes(y);
    let (wrh, wrl) = u16_bytes(wr);
    let (hh,  hl)  = u16_bytes(h);
    // le is 3 bytes for large images
    let le_b0 = ((le >> 16) & 0xFF) as u8;
    let le_b1 = ((le >>  8) & 0xFF) as u8;
    let le_b2 = ( le        & 0xFF) as u8;
    let (wh,  wl)  = u16_bytes(w);
 
    vec![
        HEADER, 0xAA, 0x16, 0x04, 0x02, 0x01, 0x50,
        xh, xl, yh, yl,
        wrh, wrl, hh, hl,
        0x00, le_b0,
        le_b1, le_b2,
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
    /// Temperature in °C. The raw packet is FF AA 0B 0B 02 <temp> FF FF FF 00 55
    /// data[5] holds the temperature byte.
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
 
/// Parse a packet that was accumulated between two FOOTER (0x55) bytes.
/// `data` is the raw bytes NOT including the terminating 0x55.
/// The leading 0xFF IS included.
pub fn parse_packet(data: &[u8]) -> Option<DeviceMessage> {
    if data.len() < 2 { return None; }
 
    // Old-style short packets: FF <cmd> ...  (no 0xAA)
    if data[0] == HEADER && data.len() >= 2 && data[1] != 0xAA {
        return match data[1] {
            0x00 => Some(DeviceMessage::Online),
            0x02 => Some(DeviceMessage::ConnectionOk),
            _    => Some(DeviceMessage::Unknown(data.to_vec())),
        };
    }
 
    if data.len() < 3 { return None; }
    if data[0] != HEADER || data[1] != 0xAA { return None; }
 
    let cmd = data[2];
 
    match cmd {
        // ── upload events:  FF AA 08 04 01 <ev> [pct] ... 55
        0x08 if data.len() >= 7 && data[3] == 0x04 && data[4] == 0x01 => {
            let ev = match data[5] {
                0x02 => UploadEvent::ImageInfoReceived,
                0x03 if data.get(6).copied().unwrap_or(0) == 0 => UploadEvent::WaitingForImage,
                0x03 => UploadEvent::UploadPercent(data[6]),
                0x04 => UploadEvent::UploadFinished,
                _ => return Some(DeviceMessage::Unknown(data.to_vec())),
            };
            Some(DeviceMessage::UploadInfo(ev))
        }
 
        // ── temperature / status:  FF AA 0B 0B 02 <temp> FF FF FF 00
        //    The packet [FF AA 0B 0B 02 1E FF FF FF 00] means 0x1E = 30 °C.
        //    Note: the 0xFF bytes inside the payload are NOT footers because
        //    the serial reader accumulates until 0x55.  The length byte (data[3])
        //    is 0x0B = 11, matching the full payload.
        0x0B if data.len() >= 6 => {
            Some(DeviceMessage::Status {
                temperature: data[5],
            })
        }
 
        // ── carving progress:  FF AA 0E ... pct xH xL yH yL ...
        0x0E if data.len() >= 11 => Some(DeviceMessage::CarvingProgress {
            percent: data[6],
            x: (data[7] as u16) << 8 | data[8] as u16,
            y: (data[9] as u16) << 8 | data[10] as u16,
        }),
 
        // ── after-upload confirmation
        0x10 if data.len() >= 15 => Some(DeviceMessage::AfterUpload {
            x: (data[7]  as u16) << 8 | data[8]  as u16,
            y: (data[9]  as u16) << 8 | data[10] as u16,
            w: (data[11] as u16) << 8 | data[12] as u16,
            h: (data[13] as u16) << 8 | data[14] as u16,
        }),
 
        _ => Some(DeviceMessage::Unknown(data.to_vec())),
    }
}
