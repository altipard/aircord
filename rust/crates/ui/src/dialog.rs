//! The system's file dialogs, as BIMCraft runs them: begun on the window's thread (AppKit wants that, and the sheet it
//! opens does not block) and waited for on a thread of their own, so the window keeps its event loop and the path comes
//! back in a later frame. A dialog run inside the event handler nests a run loop there, and on macOS a drag reaching
//! the window meanwhile re-enters the handler and aborts the app.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::Context;

use crate::App;

/// What a dialog's path is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Then {
    ImportPlaybook,
    ExportPlaybook(String),
    Firmware,
}

/// A dialog that is open.
pub(crate) struct Dialog {
    answer: Receiver<Option<PathBuf>>,
    then: Then,
}

impl App {
    /// Asks for a file to open, or for where to save one, and acts on the path in a later frame. Nothing happens on a
    /// cancel, and no second dialog opens while one is.
    pub(crate) fn ask_file(&mut self, ctx: &Context, dialog: rfd::AsyncFileDialog, save: bool, then: Then) {
        if self.dialog.is_some() {
            return;
        }
        let future: Pin<Box<dyn Future<Output = Option<rfd::FileHandle>> + Send>> =
            if save { Box::pin(dialog.save_file()) } else { Box::pin(dialog.pick_file()) };
        let (tx, answer) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let file = pollster::block_on(future);
            let _ = tx.send(file.map(|f| f.path().to_path_buf()));
            ctx.request_repaint();
        });
        self.dialog = Some(Dialog { answer, then });
    }

    /// Once a frame: acts on the path an open dialog has given. A thread that died leaves no answer.
    pub(crate) fn dialog_answered(&mut self) {
        let Some(open) = &self.dialog else { return };
        match open.answer.try_recv() {
            Ok(path) => {
                if let Some(Dialog { then, .. }) = self.dialog.take()
                    && let Some(path) = path
                {
                    self.file_chosen(then, path);
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.dialog = None,
        }
    }

    /// What a chosen path does. Tests call it directly, as the system's dialog cannot be driven.
    pub fn file_chosen(&mut self, then: Then, path: PathBuf) {
        match then {
            Then::ImportPlaybook => self.import_playbook_file(&path),
            Then::ExportPlaybook(name) => self.export_playbook_to(&name, &path),
            Then::Firmware => self.firmware_file(&path),
        }
    }
}

/// Reads at most `limit` bytes of a file; more than that is an error naming the file.
pub(crate) fn read_capped(path: &std::path::Path, limit: u64, what: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    file.take(limit + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
    if data.len() as u64 > limit {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return Err(format!("{name} is too large for {what}"));
    }
    Ok(data)
}
