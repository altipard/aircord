//! What the radio and the jobs tell the window. They run on threads of their own and post here; the window takes the
//! messages at the start of each frame, so every change to its state happens on its own thread, as the Go build
//! marshalled everything through `fyne.Do`.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use aircord_core::link::Events;
use aircord_core::{ota, playbook};
use egui::Context;

/// One thing that happened.
#[derive(Debug)]
pub enum Msg {
    Log(String),
    LinkDropped,
    ScanFailed(String),
    Connected { name: String, addr: String, profile: String },
    ConnectFailed,
    Status(String),
    Playbook(playbook::Report),
    PlaybookDone { name: String, result: Result<(), String> },
    Ota(ota::Report),
    OtaDone(Result<(), String>),
}

/// The sending side, for the link and the jobs: it wakes the window for each message.
#[derive(Clone)]
pub struct Outbox {
    tx: Sender<Msg>,
    ctx: Arc<Mutex<Option<Context>>>,
}

impl Outbox {
    pub fn post(&self, msg: Msg) {
        let _ = self.tx.send(msg);
        if let Some(ctx) = self.ctx.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            ctx.request_repaint();
        }
    }

    pub fn log(&self, line: impl Into<String>) {
        self.post(Msg::Log(line.into()));
    }
}

impl Events for Outbox {
    fn log(&self, line: &str) {
        self.post(Msg::Log(line.to_string()));
    }

    fn link_dropped(&self) {
        self.post(Msg::LinkDropped);
    }

    fn scan_failed(&self, why: &str) {
        self.post(Msg::ScanFailed(why.to_string()));
    }
}

/// The window's side.
pub struct Inbox {
    rx: Receiver<Msg>,
    outbox: Outbox,
}

impl Default for Inbox {
    fn default() -> Inbox {
        let (tx, rx) = channel();
        Inbox { rx, outbox: Outbox { tx, ctx: Arc::new(Mutex::new(None)) } }
    }
}

impl Inbox {
    /// Where to send to this inbox; give it to the link before the window exists.
    pub fn outbox(&self) -> Outbox {
        self.outbox.clone()
    }

    /// The window's context, once it has one: posting wakes it from then on.
    pub(crate) fn attach(&self, ctx: &Context) {
        let mut slot = self.outbox.ctx.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(ctx.clone());
        }
    }

    pub(crate) fn drain(&self) -> Vec<Msg> {
        self.rx.try_iter().collect()
    }
}
