//! The window driven as a user drives it (egui_kittest), over the fake link: buttons by their names, keys, typing. The
//! jobs (connect, a playbook, an update) run on their own threads as in the app, so a test waits for their outcome.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aircord_core::link::Link;
use aircord_core::link::fake::FakeLink;
use aircord_core::playbook;
use egui::Key;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::dialog::Then;
use crate::inbox::Inbox;
use crate::{App, Conn, Modal, Stores, Timing};

const IMEI: &str = "860601234567890";

struct World {
    h: Harness<'static, App>,
    link: Arc<FakeLink>,
    dir: tempfile::TempDir,
}

fn world(link: FakeLink) -> World {
    let dir = tempfile::tempdir().unwrap();
    let link = Arc::new(link);
    let inbox = Inbox::default();
    link.set_events(Arc::new(inbox.outbox()));
    let mut app = App::new(Arc::clone(&link) as Arc<dyn Link>, Stores::at(dir.path()), inbox);
    app.timing = Timing {
        settle: Duration::from_millis(10),
        reply_wait: Duration::from_millis(300),
        ota: aircord_core::ota::Timing {
            bootloader_window: Duration::from_secs(2),
            reset_drop: Duration::from_millis(200),
            poll: Duration::from_millis(5),
        },
    };
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_max_steps(16)
        .with_step_dt(0.1)
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);
    let _ = h.try_run();
    World { h, link, dir }
}

fn run(h: &mut Harness<App>) {
    let _ = h.try_run();
}

fn press(h: &mut Harness<App>, label: &str) {
    h.get_by_label(label).click();
    run(h);
}

/// Frames until `done` holds: the jobs run on threads of their own.
fn wait(h: &mut Harness<App>, what: &str, done: impl Fn(&App) -> bool) {
    for _ in 0..400 {
        run(h);
        if done(h.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for {what}; status: {}", h.state().status);
}

fn logged(app: &App, needle: &str) -> bool {
    app.log.iter().any(|l| l.text.contains(needle))
}

/// Scans, selects `name` and connects.
fn connected(w: &mut World, name: &str) {
    press(&mut w.h, "Scan");
    press(&mut w.h, "Stop");
    press(&mut w.h, name);
    press(&mut w.h, "Connect");
    wait(&mut w.h, "the connection", |a| matches!(a.conn, Conn::Connected { .. }));
}

#[test]
fn a_scan_lists_the_devices_strongest_first_and_a_header_sorts() {
    let link = FakeLink::console("aa", "D20-NB");
    link.add_nearby("bb", "Beacon", -40);
    link.add_nearby("cc", "", -90);
    let mut w = world(link);
    assert!(w.h.query_by_label("No devices yet").is_some());
    press(&mut w.h, "Scan");
    press(&mut w.h, "Stop");
    let order = |a: &App| a.display.iter().map(|d| d.addr.clone()).collect::<Vec<_>>();
    assert_eq!(order(w.h.state()), ["bb", "aa", "cc"]);
    assert!(w.h.state().status.starts_with("stopped — 3/3 shown"), "{}", w.h.state().status);
    press(&mut w.h, "Name");
    assert_eq!(order(w.h.state()), ["bb", "aa", "cc"], "Beacon, D20-NB, and the unnamed one last");
    press(&mut w.h, "Name");
    assert_eq!(order(w.h.state()), ["aa", "bb", "cc"], "reversed, unnamed still last");
    w.h.state_mut().filter = "^d2".into();
    w.h.state_mut().snapshot();
    assert_eq!(order(w.h.state()), ["aa"]);
}

#[test]
fn connect_sends_the_pin_masked_and_suggests_the_template() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    w.h.state_mut().pin = "349011".into();
    connected(&mut w, "D20-NB");
    wait(&mut w.h, "the PIN", |a| logged(a, ">> ******"));
    assert!(w.link.sent().contains(&"******".to_string()), "the fake records the echo");
    assert!(!w.h.state().log_text().contains("349011"), "the PIN never reaches the log");
    assert_eq!(w.h.state().template.as_deref(), Some("Dragino NB-IoT"));
    assert!(w.h.query_by_label("Send: Reboot device").is_some(), "the panel shows the template's actions");
}

#[test]
fn a_command_typed_is_sent_and_kept_in_the_device_history() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    connected(&mut w, "D20-NB");
    w.h.state_mut().cmd = "AT+VER".into();
    press(&mut w.h, "Send");
    assert_eq!(w.link.sent(), ["AT+VER"]);
    assert!(w.h.state().cmd.is_empty());
    wait(&mut w.h, "the reply", |a| logged(a, "<< OK"));
    let again = aircord_core::history::History::at(w.dir.path().join("history.json"));
    assert_eq!(again.get("aa"), ["AT+VER"]);
}

#[test]
fn up_opens_the_history_and_enter_runs_the_newest() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    connected(&mut w, "D20-NB");
    for cmd in ["AT+CFG", "AT+VER"] {
        w.h.state_mut().send_cmd(cmd);
    }
    w.h.state_mut().focus_cmd = true;
    run(&mut w.h);
    w.h.key_press(Key::ArrowUp);
    run(&mut w.h);
    assert!(w.h.state().picker.is_some(), "Up in the command line opens the picker");
    w.h.key_press(Key::Enter);
    run(&mut w.h);
    assert!(w.h.state().picker.is_none());
    assert_eq!(w.link.sent(), ["AT+CFG", "AT+VER", "AT+VER"]);
}

#[test]
fn a_destructive_panel_action_asks_before_it_sends() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    connected(&mut w, "D20-NB");
    press(&mut w.h, "Send: Reboot device");
    assert!(w.link.sent().is_empty(), "nothing goes out before the yes");
    assert!(w.h.query_by_label("Reboot device?").is_some());
    press(&mut w.h, "Cancel");
    assert!(w.link.sent().is_empty());
    press(&mut w.h, "Send: Reboot device");
    // The confirmation's Send, not the command line's below it.
    w.h.get_all_by_label("Send").min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().click();
    run(&mut w.h);
    assert_eq!(w.link.sent(), ["ATZ"]);
}

#[test]
fn a_read_action_shows_the_value_from_the_reply() {
    let link = FakeLink::console("aa", "D20-NB");
    link.reply("AT+VER", &["Dragino D20-NB firmware v1.4.2", "OK"]);
    let mut w = world(link);
    connected(&mut w, "D20-NB");
    press(&mut w.h, "Read: Firmware version");
    wait(&mut w.h, "the value", |a| a.values.values().any(|v| v == "1.4.2"));
}

#[test]
fn a_dropped_link_resets_the_window() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    connected(&mut w, "D20-NB");
    w.link.drop_link();
    wait(&mut w.h, "the drop", |a| a.conn == Conn::Idle);
    assert_eq!(w.h.state().status, "link dropped — reconnect");
    assert!(w.h.state().template.is_none());
}

#[test]
fn a_playbook_that_reboots_asks_first_then_runs_every_step() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    connected(&mut w, "D20-NB");
    press(&mut w.h, "Playbooks");
    press(&mut w.h, "Run");
    assert!(w.h.query_by_label("Run \"dragino-nb-1nce\"?").is_some(), "the default ends in ATZ");
    assert!(w.link.sent().is_empty());
    press(&mut w.h, "Run anyway");
    wait(&mut w.h, "the run", |a| a.running.is_none() && logged(a, "finished"));
    let steps: Vec<String> = playbook::dragino_1nce().steps.into_iter().map(|s| s.cmd).collect();
    assert_eq!(w.link.sent(), steps);
    assert!(w.h.state().modal.is_none(), "the dialog got out of the terminal's way");
}

#[test]
fn a_playbook_step_without_its_reply_aborts_the_run() {
    let link = FakeLink::console("aa", "D20-NB");
    link.reply("AT+APN=iot.1nce.net", &["ERROR"]);
    let mut w = world(link);
    connected(&mut w, "D20-NB");
    w.h.state_mut().run_playbook(playbook::dragino_1nce());
    wait(&mut w.h, "the abort", |a| a.running.is_none());
    assert!(
        w.h.state().alert.text.contains("aborted at step 2/7 (AT+APN=iot.1nce.net): no reply matching OK"),
        "{}",
        w.h.state().alert.text
    );
    assert_eq!(w.link.sent().len(), 2);
}

fn playbook_alert(app: &App) -> String {
    match &app.modal {
        Some(Modal::Playbooks(d)) => d.alert.text.clone(),
        _ => String::new(),
    }
}

#[test]
fn import_refuses_a_bad_file_and_asks_before_replacing() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    press(&mut w.h, "Playbooks");
    let file = |dir: &Path, name: &str, body: &str| {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    };
    let bad = file(w.dir.path(), "bad.json", r#"{"name":"x","steps":[]}"#);
    w.h.state_mut().file_chosen(Then::ImportPlaybook, bad);
    run(&mut w.h);
    assert_eq!(playbook_alert(w.h.state()), "import failed: playbook has no steps");

    let good = file(w.dir.path(), "good.json", r#"{"name":"Mein Playbook","steps":[{"cmd":"AT+CFG"},{"cmd":"AT+VER","waitFor":"OK"}]}"#);
    w.h.state_mut().file_chosen(Then::ImportPlaybook, good.clone());
    run(&mut w.h);
    assert_eq!(playbook_alert(w.h.state()), "imported: Mein Playbook  ·  2 steps");
    assert!(w.h.query_by_label("Mein Playbook").is_some(), "the list shows it");
    assert!(w.dir.path().join("playbooks/mein-playbook.json").exists());

    w.h.state_mut().file_chosen(Then::ImportPlaybook, good);
    run(&mut w.h);
    assert!(w.h.query_by_label("Replace playbook?").is_some(), "a taken name asks");
    press(&mut w.h, "Replace");
    assert_eq!(playbook_alert(w.h.state()), "imported: Mein Playbook  ·  2 steps");
}

#[test]
fn an_export_is_what_an_import_reads() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    press(&mut w.h, "Playbooks");
    let out = w.dir.path().join("out.json");
    w.h.state_mut().file_chosen(Then::ExportPlaybook("dragino-nb-1nce".into()), out.clone());
    run(&mut w.h);
    assert_eq!(playbook_alert(w.h.state()), "exported: dragino-nb-1nce → out.json");
    let back = playbook::decode(&std::fs::read(out).unwrap()).unwrap();
    assert_eq!(back, playbook::dragino_1nce());
}

#[test]
fn the_editor_saves_a_playbook_from_its_text() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    press(&mut w.h, "Playbooks");
    press(&mut w.h, "Editor");
    press(&mut w.h, "Save");
    assert_eq!(playbook_alert(w.h.state()), "enter a name first");
    if let Some(Modal::Playbooks(d)) = &mut w.h.state_mut().modal {
        d.name = "Quick".into();
        d.text = "# check\nAT+CFG ?OK\nAT+VER @500\n".into();
    }
    press(&mut w.h, "Save");
    assert_eq!(playbook_alert(w.h.state()), "saved: Quick");
    let pb = w.h.state().stores.playbooks.load("Quick").unwrap();
    assert_eq!(playbook::format_steps(&pb.steps), "AT+CFG ?OK\nAT+VER @500\n");
}

#[test]
fn a_session_brings_back_the_list_pin_and_target() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    press(&mut w.h, "Scan");
    press(&mut w.h, "Stop");
    press(&mut w.h, "D20-NB");
    w.h.state_mut().pin = "1234".into();
    let saved = w.h.state().capture_session("field");
    w.h.state().stores.sessions.save(&saved).unwrap();

    w.h.state_mut().clear_list();
    w.h.state_mut().pin.clear();
    let loaded = w.h.state().stores.sessions.load("field").unwrap();
    w.h.state_mut().apply_session(loaded);
    run(&mut w.h);
    let app = w.h.state();
    assert_eq!(app.pin, "1234");
    assert!(app.selected.is_none(), "a snapshot can't connect");
    assert_eq!(app.want_addr.as_deref(), Some("aa"));
    press(&mut w.h, "Scan");
    wait(&mut w.h, "the target", |a| a.selected.as_ref().is_some_and(|s| s.addr == "aa"));
    assert!(logged(w.h.state(), "session target found: D20-NB"));
}

#[test]
fn a_firmware_update_resets_the_node_and_flashes_its_bootloader() {
    let mut w = world(FakeLink::nb_node("aa", "D20-NB", IMEI));
    connected(&mut w, "D20-NB");
    press(&mut w.h, "Firmware");
    press(&mut w.h, "Read");
    wait(&mut w.h, "the IMEI", |a| matches!(&a.modal, Some(Modal::Firmware(d)) if d.imei == IMEI));
    let image: Vec<u8> = (0..3000).map(|i| (i % 253) as u8).collect();
    let bin = w.dir.path().join("fw.bin");
    std::fs::write(&bin, &image).unwrap();
    w.h.state_mut().file_chosen(Then::Firmware, bin);
    run(&mut w.h);
    press(&mut w.h, "Start update");
    assert!(w.h.query_by_label("Flash fw.bin?").is_some());
    press(&mut w.h, "Flash");
    wait(&mut w.h, "the update", |a| a.ota.as_ref().is_some_and(|r| r.done.is_some()));
    let run_ = w.h.state().ota.as_ref().map(|r| r.done.clone());
    assert_eq!(run_, Some(Some(Ok(()))), "log:\n{}", w.h.state().log_text());
    let flashed = w.link.bootloader_image();
    assert_eq!(&flashed[..image.len()], &image[..]);
    assert!(w.link.sent().iter().any(|s| s == "ATZ"));
    assert_eq!(w.h.state().conn, Conn::Idle);
    press(&mut w.h, "Close");
    assert!(w.h.state().ota.is_none());
}

#[test]
fn the_firmware_dialog_refuses_to_start_without_an_image() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    press(&mut w.h, "Firmware");
    press(&mut w.h, "Start update");
    match &w.h.state().modal {
        Some(Modal::Firmware(d)) => assert_eq!(d.alert.text, "choose a firmware image first"),
        _ => panic!("the dialog stays open"),
    }
}

#[test]
fn saving_the_log_writes_a_file_and_says_where() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    press(&mut w.h, "Save log");
    assert_eq!(w.h.state().alert.text, "log is empty — nothing to save");
    w.h.state_mut().log_line("-- hello --");
    press(&mut w.h, "Save log");
    let files: Vec<_> = std::fs::read_dir(w.dir.path().join("logs")).unwrap().flatten().collect();
    assert_eq!(files.len(), 1);
    assert!(std::fs::read_to_string(files[0].path()).unwrap().contains("-- hello --"));
}

#[test]
fn the_theme_switch_flips_between_light_and_dark() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    assert_eq!(w.h.ctx.theme(), egui::Theme::Light, "light is the default");
    press(&mut w.h, "Dark theme");
    assert_eq!(w.h.ctx.theme(), egui::Theme::Dark);
    assert!(w.h.query_by_label("Light theme").is_some(), "the switch now offers the way back");
    assert!(w.h.query_by_label("Dark theme").is_none());
    press(&mut w.h, "Light theme");
    assert_eq!(w.h.ctx.theme(), egui::Theme::Light);
    assert!(w.h.query_by_label("Dark theme").is_some());
}

#[test]
fn the_rail_names_its_dialogs() {
    let w = world(FakeLink::console("aa", "D20-NB"));
    for name in ["Sessions", "Playbooks", "Firmware"] {
        assert!(w.h.query_by_label(name).is_some(), "{name} is in the rail");
    }
}

/// Renders the window in both themes to PNGs under `$AIRCORD_SHOTS` (default: the system temp dir).
#[cfg(feature = "screenshot")]
#[test]
#[ignore = "needs a GPU or a software renderer; run with --ignored"]
fn screenshot() {
    let out = std::env::var_os("AIRCORD_SHOTS").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    for (theme, name) in [(egui::Theme::Dark, "dark"), (egui::Theme::Light, "light")] {
        let link = FakeLink::nb_node("6c1a3b52-8f0e-4d55-9a21-0e7d2c4b9f10", "D20-NB", IMEI);
        link.add_nearby("2f9b7c1e-1d3a-4c8b-8e6f-5a4b3c2d1e0f", "D23-NB", -71);
        link.add_nearby("9a8b7c6d-5e4f-4a3b-2c1d-0e9f8a7b6c5d", "WQS-NB", -83);
        link.add_nearby("1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "", -94);
        link.reply("AT+VER", &["Dragino D20-NB v1.4.2", "OK"]);
        let dir = tempfile::tempdir().unwrap();
        let link = Arc::new(link);
        let inbox = Inbox::default();
        link.set_events(Arc::new(inbox.outbox()));
        let mut app = App::new(Arc::clone(&link) as Arc<dyn Link>, Stores::at(dir.path()), inbox);
        app.timing.settle = Duration::from_millis(10);
        let mut h = Harness::builder()
            .with_size([1280.0, 780.0])
            .with_max_steps(16)
            .with_step_dt(0.1)
            .wgpu()
            .build_ui_state(|ui, app: &mut App| app.show(ui), app);
        h.ctx.set_theme(theme);
        run(&mut h);
        press(&mut h, "Scan");
        press(&mut h, "Stop");
        press(&mut h, "D20-NB");
        press(&mut h, "Connect");
        wait(&mut h, "the connection", |a| matches!(a.conn, Conn::Connected { .. }));
        for cmd in ["AT+CFG", "AT+VER"] {
            h.state_mut().send_cmd(cmd);
        }
        press(&mut h, "Read: Firmware version");
        wait(&mut h, "the value", |a| !a.values.is_empty() && !a.values.values().any(|v| v == "…"));
        h.state_mut().alert.ok("log copied to clipboard");
        run(&mut h);
        let image = h.render().unwrap();
        image.save(out.join(format!("aircord-{name}.png"))).unwrap();
        press(&mut h, "Playbooks");
        let image = h.render().unwrap();
        image.save(out.join(format!("aircord-playbooks-{name}.png"))).unwrap();
    }
}
