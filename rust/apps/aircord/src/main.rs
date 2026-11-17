//! `aircord`: the desktop app. Moves a pre-rename config directory across, opens the stores, starts the radio and
//! shows the window.

// A release build on Windows is a window, not a console program.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::process::ExitCode;
use std::sync::Arc;

use aircord_ble::BleLink;
use aircord_ui::inbox::Inbox;
use aircord_ui::{App, Stores};

fn main() -> ExitCode {
    // Before any store reads: history, sessions and playbooks of the old name survive the rename.
    aircord_core::config::migrate();
    let inbox = Inbox::default();
    let link = match BleLink::new(Arc::new(inbox.outbox())) {
        Ok(link) => link,
        Err(e) => {
            eprintln!("aircord: the radio's runtime did not start: {e}");
            return ExitCode::FAILURE;
        }
    };
    let app = App::new(Arc::new(link), Stores::open(), inbox);
    let mut viewport =
        eframe::egui::ViewportBuilder::default().with_title("Aircord").with_inner_size([1180.0, 760.0]).with_min_inner_size([900.0, 560.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../../assets/icon.png")) {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    match eframe::run_native("Aircord", options, Box::new(|_cc| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("aircord: the window could not open: {e}");
            ExitCode::FAILURE
        }
    }
}
