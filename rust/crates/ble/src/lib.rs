//! `aircord-ble`: [`Link`] over a real radio with btleplug. The radio's work runs on a tokio runtime this crate owns;
//! the window and the jobs call the [`Link`] methods from their own threads, as the Go build called its `ble.Client`
//! from goroutines. The quick calls hand work to the runtime and return; `connect` and `send_raw` wait for it, so they
//! are called from a job's thread, never the window's.
//!
//! Writes go through one writer task, in order, so the lines of a playbook reach the device as they were sent.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use aircord_core::lines::{Tap, Taps, append_lines};
use aircord_core::link::{Device, Events, Link, mask, summarize_line};
use aircord_core::redact::{Direction, log_line};
use btleplug::api::{Central, CentralEvent, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures::StreamExt;
use tokio::runtime::Runtime;
use tokio::sync::{OnceCell, mpsc};
use uuid::Uuid;

/// The BLE write chunk: the smallest ATT payload every device takes (and what Dragino's nbtool uses).
const CHUNK: usize = 20;

/// How long a connection may take, discovery included.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Nordic UART: central to device (write).
pub const NUS_RX: Uuid = Uuid::from_u128(0x6e40_0002_b5a3_f393_e0a9_e50e_24dc_ca9e);
/// Nordic UART: device to central (notify).
pub const NUS_TX: Uuid = Uuid::from_u128(0x6e40_0003_b5a3_f393_e0a9_e50e_24dc_ca9e);
/// HM-10 / TI CC254x transparent UART, write and notify on one characteristic (the Dragino D20S-NB has it).
pub const HM10: Uuid = Uuid::from_u128(0x0000_ffe1_0000_1000_8000_0080_5f9b_34fb);

/// The serial-over-BLE profiles, tried in order: a name, the write and the notify characteristic. A new device's go
/// here, read off the GATT dump in the terminal.
pub const PROFILES: &[(&str, Uuid, Uuid)] = &[("Nordic", NUS_RX, NUS_TX), ("HM-10", HM10, HM10)];

/// The first profile whose characteristics are all there: its name, write and notify UUIDs.
pub fn select_profile(has: impl Fn(&Uuid) -> bool) -> Option<(&'static str, Uuid, Uuid)> {
    PROFILES.iter().copied().find(|(_, w, n)| has(w) && has(n))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The live connection.
struct Conn {
    addr: String,
    id: PeripheralId,
    peripheral: Peripheral,
    write: Characteristic,
    /// Tells this connection's notifications apart from a later one's.
    generation: u64,
}

struct Write {
    data: Vec<u8>,
    echo: String,
    done: Option<std_mpsc::SyncSender<Result<(), String>>>,
}

struct Shared {
    events: Arc<dyn Events>,
    adapter: OnceCell<Adapter>,
    devices: Mutex<HashMap<String, (Device, PeripheralId)>>,
    scanning: AtomicBool,
    conn: Mutex<Option<Conn>>,
    generation: AtomicU64,
    taps: Taps,
}

/// The radio.
pub struct BleLink {
    runtime: Runtime,
    shared: Arc<Shared>,
    writes: mpsc::UnboundedSender<Write>,
}

impl BleLink {
    /// A link reporting to `events`. The adapter is powered on at the first scan or connect (on macOS that is when the
    /// Bluetooth permission prompt appears).
    pub fn new(events: Arc<dyn Events>) -> std::io::Result<BleLink> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("aircord-ble").enable_all().build()?;
        let shared = Arc::new(Shared {
            events,
            adapter: OnceCell::new(),
            devices: Mutex::new(HashMap::new()),
            scanning: AtomicBool::new(false),
            conn: Mutex::new(None),
            generation: AtomicU64::new(0),
            taps: Taps::default(),
        });
        let (writes, queue) = mpsc::unbounded_channel();
        runtime.spawn(writer(Arc::clone(&shared), queue));
        Ok(BleLink { runtime, shared, writes })
    }

    fn enqueue(&self, data: Vec<u8>, echo: String, done: Option<std_mpsc::SyncSender<Result<(), String>>>) -> bool {
        if !self.is_connected() {
            return false;
        }
        // The echo goes to the log through log_line, which masks a password typed or sent as a setting.
        self.writes.send(Write { data, echo, done }).is_ok()
    }
}

impl Shared {
    /// The adapter, powered on once; the first call also starts following its events.
    async fn adapter(self: &Arc<Shared>) -> Result<Adapter, String> {
        let me = Arc::clone(self);
        self.adapter
            .get_or_try_init(|| async move {
                let manager = Manager::new().await.map_err(|e| e.to_string())?;
                let adapter =
                    manager.adapters().await.map_err(|e| e.to_string())?.into_iter().next().ok_or("no Bluetooth adapter found")?;
                let events = adapter.events().await.map_err(|e| e.to_string())?;
                tokio::spawn(follow(me, adapter.clone(), events));
                Ok::<_, String>(adapter)
            })
            .await
            .cloned()
    }

    /// Takes the connection down: the peripheral to close, if there was one. Both our own Disconnect and the platform's
    /// drop event come here; only the first finds one.
    fn take_down(&self) -> Option<Peripheral> {
        let conn = lock(&self.conn).take()?;
        self.taps.close_all();
        Some(conn.peripheral)
    }

    fn write_target(&self) -> Option<(Peripheral, Characteristic)> {
        lock(&self.conn).as_ref().map(|c| (c.peripheral.clone(), c.write.clone()))
    }
}

/// Follows the adapter: discovery while scanning, and the drop of the live connection.
async fn follow(shared: Arc<Shared>, adapter: Adapter, mut events: std::pin::Pin<Box<dyn futures::Stream<Item = CentralEvent> + Send>>) {
    while let Some(event) = events.next().await {
        match event {
            CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) | CentralEvent::RssiUpdate { id, .. } => {
                if shared.scanning.load(Ordering::Relaxed) {
                    record(&shared, &adapter, id).await;
                }
            }
            CentralEvent::DeviceDisconnected(id) => {
                let ours = lock(&shared.conn).as_ref().is_some_and(|c| c.id == id);
                if ours && shared.take_down().is_some() {
                    shared.events.link_dropped();
                }
            }
            _ => {}
        }
    }
}

/// Records an advertisement. An empty name never overwrites a known one: adverts sometimes leave it out.
async fn record(shared: &Shared, adapter: &Adapter, id: PeripheralId) {
    let Ok(p) = adapter.peripheral(&id).await else { return };
    let Ok(Some(props)) = p.properties().await else { return };
    let addr = id.to_string();
    let name = props.local_name.or(props.advertisement_name).unwrap_or_default();
    let mut devices = lock(&shared.devices);
    let entry = devices.entry(addr.clone()).or_insert_with(|| (Device { addr, name: String::new(), rssi: 0, seen: Instant::now() }, id));
    if !name.is_empty() {
        entry.0.name = name;
    }
    if let Some(rssi) = props.rssi {
        entry.0.rssi = rssi;
    }
    entry.0.seen = Instant::now();
}

/// Writes queued lines in order, chunked to [`CHUNK`].
async fn writer(shared: Arc<Shared>, mut queue: mpsc::UnboundedReceiver<Write>) {
    while let Some(w) = queue.recv().await {
        let result = match shared.write_target() {
            None => Err("ble: not connected".to_string()),
            Some((p, ch)) => {
                shared.events.log(&log_line(Direction::Sent, &w.echo));
                let kind = if ch.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
                    WriteType::WithoutResponse
                } else {
                    WriteType::WithResponse
                };
                let mut result = Ok(());
                for chunk in w.data.chunks(CHUNK) {
                    if let Err(e) = p.write(&ch, chunk, kind).await {
                        shared.events.log(&format!("!! write error: {e}"));
                        result = Err(e.to_string());
                        break;
                    }
                }
                result
            }
        };
        if let Some(done) = w.done {
            let _ = done.try_send(result);
        }
    }
}

/// Connects, dumps the GATT table, matches a UART profile and turns on its notifications.
async fn connect(shared: Arc<Shared>, addr: String) -> Result<String, String> {
    let adapter = shared.adapter().await.map_err(|e| format!("enable failed: {e}"))?;
    let id = lock(&shared.devices).get(&addr).map(|(_, id)| id.clone()).ok_or("connect failed: device not seen in this scan")?;
    let p = adapter.peripheral(&id).await.map_err(|e| format!("connect failed: {e}"))?;
    p.connect().await.map_err(|e| format!("connect failed: {e}"))?;
    if let Err(e) = p.discover_services().await {
        let _ = p.disconnect().await;
        return Err(format!("discover services failed: {e}"));
    }
    let log = |l: &str| shared.events.log(l);
    log("-- GATT --");
    let mut found: HashMap<Uuid, Characteristic> = HashMap::new();
    for svc in p.services() {
        log(&format!("  svc {}", svc.uuid));
        for ch in &svc.characteristics {
            log(&format!("    chr {}", ch.uuid));
            found.insert(ch.uuid, ch.clone());
        }
    }
    let Some((profile, write, notify)) = select_profile(|u| found.contains_key(u)) else {
        let _ = p.disconnect().await;
        return Err("no known UART profile (Nordic/HM-10) in GATT above".into());
    };
    let (Some(write), Some(notify)) = (found.get(&write).cloned(), found.get(&notify).cloned()) else {
        return Err("no known UART profile (Nordic/HM-10) in GATT above".into());
    };
    let stream = match p.notifications().await {
        Ok(s) => s,
        Err(e) => {
            let _ = p.disconnect().await;
            return Err(format!("enable notifications failed: {e}"));
        }
    };
    if let Err(e) = p.subscribe(&notify).await {
        let _ = p.disconnect().await;
        return Err(format!("enable notifications failed: {e}"));
    }
    let generation = shared.generation.fetch_add(1, Ordering::Relaxed) + 1;
    *lock(&shared.conn) = Some(Conn { addr, id, peripheral: p, write, generation });
    tokio::spawn(notifications(Arc::clone(&shared), stream, notify.uuid, generation));
    Ok(profile.to_string())
}

/// Puts notify chunks back together into lines: each goes to the terminal and to the taps.
async fn notifications(
    shared: Arc<Shared>,
    mut stream: std::pin::Pin<Box<dyn futures::Stream<Item = btleplug::api::ValueNotification> + Send>>,
    uuid: Uuid,
    generation: u64,
) {
    let mut buf = Vec::new();
    while let Some(n) = stream.next().await {
        if n.uuid != uuid {
            continue;
        }
        if lock(&shared.conn).as_ref().is_none_or(|c| c.generation != generation) {
            return;
        }
        for line in append_lines(&mut buf, &n.value) {
            // AT+CFG prints AT+PWORD in clear; the tap still gets the real line, the log a masked one.
            shared.events.log(&log_line(Direction::Received, &line));
            shared.taps.feed(&line);
        }
    }
}

impl Link for BleLink {
    fn scan(&self) {
        if self.shared.scanning.swap(true, Ordering::Relaxed) {
            return;
        }
        let shared = Arc::clone(&self.shared);
        self.runtime.spawn(async move {
            let started = match shared.adapter().await {
                Ok(adapter) => adapter.start_scan(ScanFilter::default()).await.map_err(|e| format!("scan failed: {e}")),
                Err(e) => Err(format!("enable failed: {e}")),
            };
            if let Err(e) = started {
                shared.scanning.store(false, Ordering::Relaxed);
                shared.events.scan_failed(&e);
            }
        });
    }

    fn stop_scan(&self) {
        if !self.shared.scanning.swap(false, Ordering::Relaxed) {
            return;
        }
        let shared = Arc::clone(&self.shared);
        self.runtime.spawn(async move {
            if let Some(adapter) = shared.adapter.get() {
                let _ = adapter.stop_scan().await;
            }
        });
    }

    fn is_scanning(&self) -> bool {
        self.shared.scanning.load(Ordering::Relaxed)
    }

    fn devices(&self) -> Vec<Device> {
        lock(&self.shared.devices).values().map(|(d, _)| d.clone()).collect()
    }

    fn clear_devices(&self) {
        lock(&self.shared.devices).clear();
    }

    fn connect(&self, addr: &str) -> Option<String> {
        let shared = Arc::clone(&self.shared);
        let addr = addr.to_string();
        let result = self.runtime.block_on(async move {
            tokio::time::timeout(CONNECT_TIMEOUT, connect(shared, addr)).await.unwrap_or_else(|_| Err("connect failed: timed out".into()))
        });
        match result {
            Ok(profile) => Some(profile),
            Err(e) => {
                self.shared.events.log(&format!("!! {e}"));
                None
            }
        }
    }

    fn disconnect(&self) -> bool {
        let Some(p) = self.shared.take_down() else { return false };
        self.runtime.spawn(async move {
            let _ = p.disconnect().await;
        });
        true
    }

    fn is_connected(&self) -> bool {
        lock(&self.shared.conn).is_some()
    }

    fn current_addr(&self) -> Option<String> {
        lock(&self.shared.conn).as_ref().map(|c| c.addr.clone())
    }

    fn send(&self, cmd: &str) {
        self.enqueue(format!("{cmd}\r\n").into_bytes(), cmd.to_string(), None);
    }

    fn send_secret(&self, cmd: &str) {
        self.enqueue(format!("{cmd}\r\n").into_bytes(), mask(cmd), None);
    }

    fn send_raw(&self, line: &str) -> Result<(), String> {
        let (done, wait) = std_mpsc::sync_channel(1);
        if !self.enqueue(line.as_bytes().to_vec(), summarize_line(line), Some(done)) {
            return Err("ble: not connected".into());
        }
        wait.recv().unwrap_or_else(|_| Err("ble: writer stopped".into()))
    }

    fn lines(&self) -> Tap {
        self.shared.taps.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nordic_wins_when_both_profiles_are_there() {
        assert_eq!(select_profile(|_| true).map(|p| p.0), Some("Nordic"));
    }

    #[test]
    fn hm10_alone() {
        assert_eq!(select_profile(|u| *u == HM10).map(|p| (p.0, p.1, p.2)), Some(("HM-10", HM10, HM10)));
    }

    #[test]
    fn nordic_needs_both_characteristics() {
        assert!(select_profile(|u| *u == NUS_RX).is_none());
        assert_eq!(select_profile(|u| *u == NUS_RX || *u == NUS_TX).map(|p| p.0), Some("Nordic"));
    }

    #[test]
    fn the_uuids_are_the_well_known_ones() {
        assert_eq!(HM10.to_string(), "0000ffe1-0000-1000-8000-00805f9b34fb");
        assert_eq!(NUS_RX.to_string(), "6e400002-b5a3-f393-e0a9-e50e24dcca9e");
    }
}
