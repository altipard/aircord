//! Device output as lines: notify chunks put back together, and the taps that let a waiting playbook or a firmware
//! update see each reply line without going through the window.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// Appends `data` to `buf` and takes out every complete `\n`-terminated line, a trailing `\r` trimmed. What is left
/// stays in `buf` for the next chunk.
pub fn append_lines(buf: &mut Vec<u8>, data: &[u8]) -> Vec<String> {
    buf.extend_from_slice(data);
    let mut lines = Vec::new();
    while let Some(i) = buf.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = buf.drain(..=i).collect();
        let text = String::from_utf8_lossy(&line[..i]);
        lines.push(text.trim_end_matches('\r').to_string());
    }
    lines
}

/// A lock that a thread which panicked while holding it does not poison for everyone else.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

type Subscribers = Arc<Mutex<Vec<(u64, SyncSender<String>)>>>;

/// Fans every reply line out to whoever is listening. A link feeds it from its notify handler, which must never block,
/// so a listener that falls behind loses lines rather than stalling the radio: the protocols here are lock-step and
/// only ever need the fresh ones.
#[derive(Clone, Default)]
pub struct Taps {
    subscribers: Subscribers,
    next: Arc<AtomicU64>,
}

/// How many lines a tap holds before it drops new ones.
pub const TAP_DEPTH: usize = 256;

impl Taps {
    /// Starts listening; the tap stops when it is dropped.
    pub fn subscribe(&self) -> Tap {
        let (tx, rx) = sync_channel(TAP_DEPTH);
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.subscribers).push((id, tx));
        Tap { rx, id, subscribers: Arc::clone(&self.subscribers) }
    }

    /// Hands `line` to every tap, without waiting.
    pub fn feed(&self, line: &str) {
        for (_, tx) in lock(&self.subscribers).iter() {
            let _ = tx.try_send(line.to_string());
        }
    }

    /// Ends every tap, as a dropped link does: a waiting reader sees it at once instead of at its timeout.
    pub fn close_all(&self) {
        lock(&self.subscribers).clear();
    }
}

/// One listener's reply lines.
pub struct Tap {
    rx: Receiver<String>,
    id: u64,
    subscribers: Subscribers,
}

/// Why no line came.
#[derive(Debug, PartialEq, Eq)]
pub enum NoLine {
    Timeout,
    /// The link dropped and closed its taps.
    Closed,
}

impl Tap {
    /// The next line, waiting at most `timeout`.
    pub fn recv(&self, timeout: Duration) -> Result<String, NoLine> {
        self.rx.recv_timeout(timeout).map_err(|e| match e {
            RecvTimeoutError::Timeout => NoLine::Timeout,
            RecvTimeoutError::Disconnected => NoLine::Closed,
        })
    }

    /// Throws away what has come so far, so an earlier reply cannot be taken for the next command's.
    pub fn drain(&self) {
        while self.rx.try_recv().is_ok() {}
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        let id = self.id;
        lock(&self.subscribers).retain(|(i, _)| *i != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_line_waits_for_the_rest() {
        let mut buf = Vec::new();
        assert!(append_lines(&mut buf, b"AT+C").is_empty());
        assert_eq!(append_lines(&mut buf, b"FG\r\nOK\n+tail"), ["AT+CFG", "OK"]);
        assert_eq!(buf, b"+tail");
    }

    #[test]
    fn many_lines_in_one_chunk() {
        let mut buf = Vec::new();
        assert_eq!(append_lines(&mut buf, b"a\nb\r\n\nc\n"), ["a", "b", "", "c"]);
        assert!(buf.is_empty());
    }

    #[test]
    fn taps_fan_out_and_unsubscribe_on_drop() {
        let taps = Taps::default();
        let a = taps.subscribe();
        let b = taps.subscribe();
        taps.feed("OK");
        assert_eq!(a.recv(Duration::from_millis(10)).unwrap(), "OK");
        assert_eq!(b.recv(Duration::from_millis(10)).unwrap(), "OK");
        drop(a);
        assert_eq!(lock(&taps.subscribers).len(), 1);
    }

    #[test]
    fn a_full_tap_drops_lines_without_blocking() {
        let taps = Taps::default();
        let tap = taps.subscribe();
        for i in 0..TAP_DEPTH + 10 {
            taps.feed(&i.to_string());
        }
        tap.drain();
        assert_eq!(tap.recv(Duration::from_millis(1)), Err(NoLine::Timeout));
    }

    #[test]
    fn closing_wakes_a_reader() {
        let taps = Taps::default();
        let tap = taps.subscribe();
        taps.close_all();
        assert_eq!(tap.recv(Duration::from_secs(5)), Err(NoLine::Closed));
    }
}
