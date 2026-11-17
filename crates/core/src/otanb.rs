//! The Dragino NB-IoT bootloader firmware-update protocol, over the serial-over-BLE channel the console uses.
//!
//! Each command is a binary frame, `START(0xFE) | cmd | password(8) | data_len(u16 LE) | data | crc32(u32 LE) |
//! END(0xEF)`, the CRC being IEEE CRC32 (zlib's) over START through data. It travels as the ASCII line
//! `AT+TX=<len>,<HEX>\r\n`; the device answers with a line holding `AT+DATA=<HEX>`, a frame of the same shape whose
//! byte 1 is a status (0 = OK). This mirrors Dragino's own open-source updater (github.com/dragino/MeshNode,
//! tool/FirmwareUpdateUtility), the reference for every constant here.
//!
//! Upgrade: ERASE, FLASH chunks (at most 2032 bytes, a multiple of 8), VERIFY address, size and CRC, REBOOT; one
//! round-trip each, and a non-zero status aborts. The bootloader region is never written, so a failed run can be retried.
//!
//! Flash layout of the STM32L072 host MCU. A `.bin` is the raw application image from [`APP_START`] on, without the
//! bootloader; the application start is 8-byte and page (0x800) aligned.
//!
//! ```text
//! 0x08000000..0x080077FF  bootloader        (never written)
//! 0x08007800..0x0803DFFF  application       (0x36800 bytes)
//! 0x0803E000..0x0803E7FF  factory identity  (protected)
//! 0x0803F000..            boot control
//! ```
//!
//! Not yet verified against hardware: the frame format, commands, CRC, layout and the `AT+TX`/`AT+DATA` transport come
//! from Dragino's shared STM32 bootloader tool, so whether an NB node takes `AT+TX` or a transport verb of its own wants
//! confirming on a real device before a production flash. Entering the bootloader (a reset within an 8 s / 12 s BLE
//! window, after which the node advertises under its IMEI) is up to the radio and window layers, not this module. The
//! protocol itself is tested end to end against a frame-accurate fake device.

use std::fmt;
use std::time::Duration;

const START: u8 = 0xFE;
const END: u8 = 0xEF;

pub const CMD_FLASH: u8 = 3;
pub const CMD_ERASE: u8 = 4;
pub const CMD_VERIFY: u8 = 5;
pub const CMD_REBOOT: u8 = 12;

pub const STATUS_OK: u8 = 0;

const FLASH_PAGE: usize = 0x800;
/// Where the application image starts; the host never writes below it.
pub const APP_START: u32 = 0x0800_7800;
/// The factory-identity boundary the application must stay below.
const APP_END: u32 = 0x0803_E000;
const APP_MAX: usize = (APP_END - APP_START) as usize;
/// START + cmd + password(8) + len(2) + crc(4) + END, with no data.
const MIN_FRAME: usize = 17;

/// The largest FLASH payload the bootloader takes in one frame.
pub const MAX_CHUNK: usize = 2032;
/// The reference tool's FLASH payload size.
pub const DEFAULT_CHUNK: usize = 224;

/// The byte the NB bootloader right-pads a short PIN with (from its changelog).
const PIN_PAD: u8 = b'O';

/// A malformed frame, a refused request, or a failed round-trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err<T>(msg: impl Into<String>) -> Result<T, Error> {
    Err(Error(format!("otanb: {}", msg.into())))
}

/// The fixed password of a node without a PIN (what Dragino's LA66/mesh tool sends).
pub fn default_password() -> [u8; 8] {
    [0x66; 8]
}

/// A device AT PIN as the 8-byte password field, right-padded with `O`.
pub fn encode_pin(pin: &str) -> Result<[u8; 8], Error> {
    let bytes = pin.as_bytes();
    if bytes.len() > 8 {
        return err(format!("PIN {} chars exceeds 8-byte password field", bytes.len()));
    }
    let mut out = [PIN_PAD; 8];
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(out)
}

fn crc(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

/// One command frame.
pub fn build_frame(cmd: u8, data: &[u8], password: &[u8; 8]) -> Result<Vec<u8>, Error> {
    let Ok(len) = u16::try_from(data.len()) else { return err(format!("data too long: {}", data.len())) };
    let mut out = Vec::with_capacity(MIN_FRAME + data.len());
    out.push(START);
    out.push(cmd);
    out.extend_from_slice(password);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(data);
    let sum = crc(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    out.push(END);
    Ok(out)
}

/// A parsed reply frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u8,
    pub data: Vec<u8>,
}

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Checks a frame's delimiters, length and CRC; its byte 1 and data.
pub fn parse_frame(frame: &[u8]) -> Result<Response, Error> {
    if frame.len() < MIN_FRAME {
        return err(format!("frame too short ({})", frame.len()));
    }
    if frame[0] != START {
        return err("bad start byte");
    }
    let n = usize::from(le16(&frame[10..12]));
    if frame.len() != MIN_FRAME + n {
        return err(format!("length mismatch: got {} want {}", frame.len(), MIN_FRAME + n));
    }
    if frame[frame.len() - 1] != END {
        return err("bad end byte");
    }
    let (got, want) = (crc(&frame[..12 + n]), le32(&frame[12 + n..16 + n]));
    if got != want {
        return err(format!("crc mismatch: got {got:08X} want {want:08X}"));
    }
    Ok(Response { status: frame[1], data: frame[12..12 + n].to_vec() })
}

fn hex_upper(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 0xF)]));
    }
    s
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text.bytes().map(|c| char::from(c).to_digit(16).and_then(|d| u8::try_from(d).ok())).collect::<Option<_>>()?;
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    Some(digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect())
}

/// A frame as the `AT+TX=<len>,<HEX>\r\n` transport line.
pub fn at_tx(packet: &[u8]) -> String {
    format!("AT+TX={},{}\r\n", packet.len(), hex_upper(packet))
}

/// The reply frame in a notify line holding `AT+DATA=<HEX>`; characters other than hex digits after the marker are
/// ignored, as the device pads the line.
pub fn parse_at_data(line: &str) -> Result<Response, Error> {
    const MARKER: &str = "AT+DATA=";
    let Some(i) = line.find(MARKER) else { return err("not an AT+DATA line") };
    let hex: String = line[i + MARKER.len()..].chars().filter(char::is_ascii_hexdigit).collect();
    match unhex(&hex) {
        Some(raw) if !raw.is_empty() => parse_frame(&raw),
        _ => err("bad AT+DATA hex"),
    }
}

/// The command and data of an `AT+TX` request line: what a device (or a fake of one) reads.
pub fn decode_request(line: &str) -> Option<(u8, Vec<u8>)> {
    let rest = line.split_once("AT+TX=")?.1.trim();
    let raw = unhex(rest.split_once(',')?.1)?;
    if raw.len() < MIN_FRAME {
        return None;
    }
    let n = usize::from(le16(&raw[10..12]));
    Some((raw[1], raw.get(12..12 + n)?.to_vec()))
}

/// A checked application image, ready to flash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firmware {
    /// The image, padded to 8 bytes with the erased-flash value.
    pub image: Vec<u8>,
    pub addr: u32,
}

impl Firmware {
    /// The checksum VERIFY compares against.
    pub fn crc32(&self) -> u32 {
        crc(&self.image)
    }
}

/// Checks a raw `.bin` application image and pads it to 8 bytes. An empty image is refused, and so is one too big for
/// the application region: a strong sign the `.bin` still carries the bootloader.
pub fn load_firmware(image: &[u8]) -> Result<Firmware, Error> {
    if image.is_empty() {
        return err("empty firmware image");
    }
    if image.len() > APP_MAX {
        return err(format!("image {} bytes exceeds application region {APP_MAX} — bootloader still in this .bin?", image.len()));
    }
    let mut padded = image.to_vec();
    padded.resize(image.len().div_ceil(8) * 8, 0xFF);
    Ok(Firmware { image: padded, addr: APP_START })
}

/// Everything an upgrade does, worked out in advance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub firmware: Firmware,
    pub password: [u8; 8],
    pub chunk: usize,
    pub erase_first: bool,
    pub reboot_after: bool,
}

/// An upgrade with the reference tool's defaults: erase, flash at `chunk` (0 for [`DEFAULT_CHUNK`]), verify, reboot.
/// The chunk must be a multiple of 8 up to [`MAX_CHUNK`].
pub fn plan_upgrade(firmware: Firmware, password: [u8; 8], chunk: usize) -> Result<Plan, Error> {
    let chunk = if chunk == 0 { DEFAULT_CHUNK } else { chunk };
    if chunk > MAX_CHUNK || !chunk.is_multiple_of(8) {
        return err(format!("chunk size {chunk} must be a multiple of 8 in 1..{MAX_CHUNK}"));
    }
    Ok(Plan { firmware, password, chunk, erase_first: true, reboot_after: true })
}

/// One request and its reply over the link: what an upgrade needs of it.
pub trait Transport {
    /// Writes one transport line ([`at_tx`]) and returns the next `AT+DATA` reply line, or why there is none.
    fn roundtrip(&mut self, line: &str, timeout: Duration) -> Result<String, String>;
}

const ERASE_TIMEOUT: Duration = Duration::from_secs(12);
const FLASH_TIMEOUT: Duration = Duration::from_secs(4);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(8);
const REBOOT_TIMEOUT: Duration = Duration::from_secs(2);

fn u32_of(n: usize) -> Result<u32, Error> {
    u32::try_from(n).or_else(|_| err("size out of range"))
}

/// Runs the whole flash sequence of `plan`, reporting bytes written of the total after each block. It stops at the first
/// refusal or failed round-trip, leaving the node in its bootloader.
pub fn upgrade(t: &mut dyn Transport, plan: &Plan, progress: &mut dyn FnMut(usize, usize)) -> Result<(), Error> {
    let fw = &plan.firmware;
    let total = fw.image.len();
    let words = |values: &[u32]| values.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();

    if plan.erase_first {
        let size = u32_of(total.div_ceil(FLASH_PAGE) * FLASH_PAGE)?;
        roundtrip_ok(t, CMD_ERASE, &words(&[fw.addr, size]), &plan.password, ERASE_TIMEOUT, "erase")?;
    }
    let mut sent = 0;
    for block in fw.image.chunks(plan.chunk) {
        let addr = fw.addr + u32_of(sent)?;
        let mut data = words(&[addr, u32_of(block.len())?]);
        data.extend_from_slice(block);
        roundtrip_ok(t, CMD_FLASH, &data, &plan.password, FLASH_TIMEOUT, &format!("flash@0x{addr:08X}"))?;
        sent += block.len();
        progress(sent, total);
    }
    roundtrip_ok(t, CMD_VERIFY, &words(&[fw.addr, u32_of(total)?, fw.crc32()]), &plan.password, VERIFY_TIMEOUT, "verify")?;
    if plan.reboot_after {
        roundtrip_ok(t, CMD_REBOOT, &[0], &plan.password, REBOOT_TIMEOUT, "reboot")?;
    }
    Ok(())
}

fn roundtrip_ok(t: &mut dyn Transport, cmd: u8, data: &[u8], password: &[u8; 8], timeout: Duration, what: &str) -> Result<(), Error> {
    let frame = build_frame(cmd, data, password)?;
    let reply = t.roundtrip(&at_tx(&frame), timeout).or_else(|e| err(format!("{what} send failed: {e}")))?;
    let resp = parse_at_data(&reply).or_else(|e| err(format!("{what} reply unparseable: {e}")))?;
    if resp.status != STATUS_OK {
        return err(format!("{what} rejected with status {}", resp.status));
    }
    Ok(())
}

/// A frame-accurate bootloader: it stores FLASH payloads and checks the CRC on VERIFY. Used by the tests here and by the
/// fake link, so the whole update runs end to end without a radio.
#[cfg(any(test, feature = "testing"))]
#[derive(Default)]
pub struct FakeBootloader {
    pub image: std::collections::BTreeMap<u32, u8>,
    pub erased: bool,
    pub rebooted: bool,
    /// Non-zero: every reply carries this status.
    pub force_status: u8,
}

#[cfg(any(test, feature = "testing"))]
impl FakeBootloader {
    /// The reply line to an `AT+TX` request line, or `None` for anything else.
    pub fn answer(&mut self, line: &str) -> Option<String> {
        let (cmd, data) = decode_request(line)?;
        let status = if self.force_status != 0 { self.force_status } else { self.handle(cmd, &data) };
        let reply = build_frame(status, &[], &default_password()).ok()?;
        Some(format!("AT+DATA={}", hex_upper(&reply)))
    }

    fn handle(&mut self, cmd: u8, data: &[u8]) -> u8 {
        match cmd {
            CMD_ERASE => self.erased = true,
            CMD_FLASH if data.len() >= 8 => {
                let (addr, n) = (le32(&data[0..4]), le32(&data[4..8]) as usize);
                if n != data.len() - 8 {
                    return 12;
                }
                for (i, b) in data[8..].iter().enumerate() {
                    self.image.insert(addr + i as u32, *b);
                }
            }
            CMD_VERIFY if data.len() >= 12 => {
                let (addr, size, want) = (le32(&data[0..4]), le32(&data[4..8]), le32(&data[8..12]));
                let bytes: Vec<u8> = (0..size).map(|i| self.image.get(&(addr + i)).copied().unwrap_or(0)).collect();
                if crc(&bytes) != want {
                    return 17;
                }
            }
            CMD_REBOOT => self.rebooted = true,
            _ => {}
        }
        STATUS_OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_password_and_a_pin() {
        assert_eq!(default_password(), [0x66; 8]);
        assert_eq!(&encode_pin("349011").unwrap(), b"349011OO");
        assert_eq!(&encode_pin("12345678").unwrap(), b"12345678");
        assert!(encode_pin("123456789").is_err());
    }

    #[test]
    fn a_frame_has_the_documented_layout() {
        let f = build_frame(CMD_REBOOT, &[0], &default_password()).unwrap();
        assert_eq!(f.len(), 18);
        assert_eq!((f[0], f[1], f[17]), (START, CMD_REBOOT, END));
        assert_eq!(le16(&f[10..12]), 1);
        assert_eq!(le32(&f[13..17]), crc32fast::hash(&f[..13]));
    }

    #[test]
    fn a_frame_round_trips_and_corruption_is_caught() {
        let payload = [0xDE, 0xAD, 0xBE, 0xEF, 0x11, 0x22];
        let f = build_frame(STATUS_OK, &payload, &default_password()).unwrap();
        assert_eq!(parse_frame(&f).unwrap(), Response { status: 0, data: payload.to_vec() });
        let mut bad = f.clone();
        bad[5] ^= 0xFF;
        assert!(parse_frame(&bad).is_err());
        assert!(parse_frame(&f[..MIN_FRAME - 1]).is_err());
    }

    #[test]
    fn the_transport_lines() {
        assert_eq!(at_tx(&[0xFE, 0x0C, 0xEF]), "AT+TX=3,FE0CEF\r\n");
        let f = build_frame(STATUS_OK, &[0xAB], &default_password()).unwrap();
        let line = format!("<< AT+DATA={}\r", hex_upper(&f));
        assert_eq!(parse_at_data(&line).unwrap(), Response { status: 0, data: vec![0xAB] });
        assert!(parse_at_data("random log line").is_err());
        assert!(parse_at_data("AT+DATA=ABC").is_err());
    }

    #[test]
    fn firmware_is_checked_and_padded() {
        assert!(load_firmware(&[]).is_err());
        assert!(load_firmware(&vec![0; APP_MAX + 1]).is_err());
        let src = [1u8, 2, 3];
        let fw = load_firmware(&src).unwrap();
        assert_eq!(fw.image, [1, 2, 3, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(fw.addr, APP_START);
        assert_eq!(src, [1, 2, 3]);
        assert_eq!(load_firmware(&[0; 16]).unwrap().image.len(), 16);
    }

    #[test]
    fn a_plan_takes_its_defaults_and_refuses_bad_chunks() {
        let fw = load_firmware(&[0; 8]).unwrap();
        let p = plan_upgrade(fw.clone(), default_password(), 0).unwrap();
        assert_eq!((p.chunk, p.erase_first, p.reboot_after), (DEFAULT_CHUNK, true, true));
        assert!(plan_upgrade(fw.clone(), default_password(), 12).is_err());
        assert!(plan_upgrade(fw, default_password(), MAX_CHUNK + 8).is_err());
    }

    struct Device(FakeBootloader);

    impl Transport for Device {
        fn roundtrip(&mut self, line: &str, _: Duration) -> Result<String, String> {
            self.0.answer(line).ok_or_else(|| "no reply".to_string())
        }
    }

    #[test]
    fn an_upgrade_runs_end_to_end() {
        let src: Vec<u8> = (0..DEFAULT_CHUNK * 2 + 5).map(|i| (i * 7 + 1) as u8).collect();
        let plan = plan_upgrade(load_firmware(&src).unwrap(), default_password(), 0).unwrap();
        let mut dev = Device(FakeBootloader::default());
        let mut last = (0, 0);
        upgrade(&mut dev, &plan, &mut |s, t| last = (s, t)).unwrap();
        let n = plan.firmware.image.len();
        assert!(dev.0.erased && dev.0.rebooted);
        assert_eq!(last, (n, n));
        for (i, b) in plan.firmware.image.iter().enumerate() {
            assert_eq!(dev.0.image.get(&(APP_START + i as u32)), Some(b));
        }
    }

    #[test]
    fn a_refusal_aborts_and_names_the_status() {
        let plan = plan_upgrade(load_firmware(&[1; 8]).unwrap(), default_password(), 0).unwrap();
        let mut dev = Device(FakeBootloader { force_status: 16, ..FakeBootloader::default() });
        let e = upgrade(&mut dev, &plan, &mut |_, _| {}).unwrap_err();
        assert!(e.0.contains("status 16"), "{e}");
    }

    struct Broken;

    impl Transport for Broken {
        fn roundtrip(&mut self, _: &str, _: Duration) -> Result<String, String> {
            Err("boom".into())
        }
    }

    #[test]
    fn a_transport_failure_surfaces() {
        let plan = plan_upgrade(load_firmware(&[1; 8]).unwrap(), default_password(), 0).unwrap();
        let e = upgrade(&mut Broken, &plan, &mut |_, _| {}).unwrap_err();
        assert!(e.0.contains("boom"), "{e}");
    }
}
