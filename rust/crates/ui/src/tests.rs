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
use crate::form::Edit;
use crate::guided::{NO_NETWORK_HINT, Read};
use crate::inbox::Inbox;
use crate::xfer::Stage;
use crate::{App, Conn, Modal, Stores, Timing, View};

const IMEI: &str = "860601234567890";

struct World {
    h: Harness<'static, App>,
    link: Arc<FakeLink>,
    dir: tempfile::TempDir,
    /// How many commands the connect itself sent: the PIN, and `AT+CFG` for the settings.
    base: usize,
}

impl World {
    /// What was sent since the connect.
    fn sent(&self) -> Vec<String> {
        self.link.sent().split_off(self.base)
    }
}

/// The expert view over `link`, as the window was before the guided view came.
fn world(link: FakeLink) -> World {
    world_with(link, &[])
}

/// The guided view over `link`: what the app opens with.
fn lay(link: FakeLink, templates: &[(&str, &str)]) -> World {
    let mut w = world_with(link, templates);
    w.h.state_mut().view = View::Guided;
    run(&mut w.h);
    w
}

/// A real S31-NB's `AT+CFG` dump and start lines.
const S31_CFG: &str = include_str!("../../core/testdata/s31-nb-v1.4.0-cfg.txt");
const S31_BOOT: &str = include_str!("../../core/testdata/s31-nb-v1.4.0-boot.txt");
/// The password in that dump.
const S31_PASSWORD: &str = "kxqmzp";
/// An NB node advertises its IMEI, not its model.
const S31_NAME: &str = "866207050000001";

fn s31() -> FakeLink {
    FakeLink::configured("aa", S31_NAME, S31_CFG)
}

/// Searches, picks `name` and connects in the guided view, and waits for the settings.
fn lay_connected(w: &mut World, name: &str) {
    press(&mut w.h, "Suchen");
    press(&mut w.h, "Stopp");
    press_card(&mut w.h, name);
    press(&mut w.h, "Verbinden");
    settled(w);
}

/// Presses the device card of `name`: its label goes on with the signal or with "verbunden".
fn press_card(h: &mut Harness<App>, name: &str) {
    h.get_by_label_contains(&format!("{name}, ")).click();
    run(h);
}

/// Waits for the connection and for the settings read that follows it.
fn settled(w: &mut World) {
    wait(&mut w.h, "the connection", |a| matches!(a.conn, Conn::Connected { .. }));
    wait(&mut w.h, "the settings", |a| matches!(a.guided.read, Read::Ready | Read::Failed(_)));
    w.base = w.link.sent().len();
}

fn has(w: &World, label: &str) -> bool {
    w.h.query_all_by_label(label).next().is_some()
}

fn has_part(w: &World, part: &str) -> bool {
    w.h.query_all_by_label_contains(part).next().is_some()
}

/// A device type of the test's own, so a test does not lean on what a preset happens to hold: picked by the name
/// `X1…`, with a secret that is not one of the always-masked keys.
const X1: &str = "name = \"Prüfgerät X1\"\nmatch = '^X1'\nmodel = '^X1-NB(,|$)'\n\
    [[action]]\nlabel = \"Firmware\"\ncmd = \"AT+FW\"\ntype = \"read\"\nparse = '([0-9]+\\.[0-9]+\\.[0-9]+)'\n\
    [[action]]\nlabel = \"Neu starten\"\ncmd = \"ATZ\"\ntype = \"button\"\nconfirm = true\n\
    [[setting]]\nkey = \"TOKEN\"\nlabel = \"Zugangsschlüssel\"\ngroup = \"Server\"\ntype = \"secret\"\n\
    [[setting]]\nkey = \"TDC\"\nlabel = \"Sendeintervall\"\ngroup = \"Senden\"\ntype = \"duration\"\nunit = \"s\"\npresets = [900, 3600]";

const X1_CFG: &str = "AT+MODEL=X1-NB,v1.0.0\nAT+TOKEN=tok-abc\nAT+TDC=900\nOK";

/// [`world`] with these files in the user's templates folder.
fn world_with(link: FakeLink, templates: &[(&str, &str)]) -> World {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("templates")).unwrap();
    for (file, text) in templates {
        std::fs::write(dir.path().join("templates").join(file), text).unwrap();
    }
    let link = Arc::new(link);
    let inbox = Inbox::default();
    link.set_events(Arc::new(inbox.outbox()));
    let mut app = App::new(Arc::clone(&link) as Arc<dyn Link>, Stores::at(dir.path()), inbox);
    app.timing = Timing {
        settle: Duration::from_millis(10),
        reply_wait: Duration::from_millis(300),
        read_wait: Duration::from_millis(300),
        no_network_hint: Duration::from_secs(60),
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
    h.state_mut().view = View::Expert;
    let _ = h.try_run();
    World { h, link, dir, base: 0 }
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
    settled(w);
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
    let mut w = world_with(FakeLink::console("aa", "X1-NB"), &[("x1.toml", X1)]);
    w.h.state_mut().pin = "349011".into();
    connected(&mut w, "X1-NB");
    wait(&mut w.h, "the PIN", |a| logged(a, ">> ******"));
    assert!(w.link.sent().contains(&"******".to_string()), "the fake records the echo");
    assert!(!w.h.state().log_text().contains("349011"), "the PIN never reaches the log");
    assert_eq!(w.h.state().template.as_deref(), Some("Prüfgerät X1"));
    assert!(w.h.query_by_label("Send: Neu starten").is_some(), "the panel shows the template's actions");
}

#[test]
fn a_command_typed_is_sent_and_kept_in_the_device_history() {
    let mut w = world(FakeLink::console("aa", "D20-NB"));
    connected(&mut w, "D20-NB");
    w.h.state_mut().cmd = "AT+VER".into();
    press(&mut w.h, "Send");
    assert_eq!(w.sent(), ["AT+VER"]);
    assert!(w.h.state().cmd.is_empty());
    wait(&mut w.h, "the reply", |a| logged(a, "<< OK"));
    let again = aircord_core::history::History::at(w.dir.path().join("history.json"));
    assert_eq!(again.get("aa"), ["AT+VER"]);
}

#[test]
fn a_template_s_own_secret_is_masked_in_the_terminal_and_kept_out_of_the_history() {
    let mut w = world_with(FakeLink::configured("aa", "X1-NB", X1_CFG), &[("x1.toml", X1)]);
    connected(&mut w, "X1-NB");
    assert_eq!(w.h.state().template.as_deref(), Some("Prüfgerät X1"));
    w.h.state_mut().send_cmd("AT+CFG");
    wait(&mut w.h, "the dump", |a| logged(a, "<< AT+TDC=900"));
    assert!(logged(w.h.state(), "<< AT+TOKEN=******"), "{}", w.h.state().log_text());
    w.h.state_mut().cmd = "AT+TOKEN=neu-123".into();
    press(&mut w.h, "Send");
    w.h.state_mut().cmd = "AT+PWORD=geheim".into();
    press(&mut w.h, "Send");
    wait(&mut w.h, "the echoes", |a| logged(a, ">> AT+PWORD=******") && logged(a, ">> AT+TOKEN=******"));
    assert_eq!(w.link.cfg_value("TOKEN").as_deref(), Some("neu-123"), "the device got the value");
    let text = w.h.state().log_text();
    assert!(!text.contains("tok-abc") && !text.contains("neu-123") && !text.contains("geheim"), "{text}");
    let again = aircord_core::history::History::at(w.dir.path().join("history.json"));
    assert_eq!(again.get("aa"), ["AT+CFG"], "a command with a password is not written to disk");
    w.h.state_mut().open_picker();
    assert_eq!(w.h.state().picker_items(), ["AT+CFG"], "nor offered to run again");
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
    assert_eq!(w.sent(), ["AT+CFG", "AT+VER", "AT+VER"]);
}

#[test]
fn a_destructive_panel_action_asks_before_it_sends() {
    let mut w = world_with(FakeLink::console("aa", "X1-NB"), &[("x1.toml", X1)]);
    connected(&mut w, "X1-NB");
    press(&mut w.h, "Send: Neu starten");
    assert!(w.sent().is_empty(), "nothing goes out before the yes");
    assert!(w.h.query_by_label("Neu starten?").is_some());
    press(&mut w.h, "Cancel");
    assert!(w.sent().is_empty());
    press(&mut w.h, "Send: Neu starten");
    // The confirmation's Send, not the command line's below it.
    w.h.get_all_by_label("Send").min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().click();
    run(&mut w.h);
    assert_eq!(w.sent(), ["ATZ"]);
}

#[test]
fn a_read_action_shows_the_value_from_the_reply() {
    let link = FakeLink::console("aa", "X1-NB");
    link.reply("AT+FW", &["X1-NB firmware v1.4.2", "OK"]);
    let mut w = world_with(link, &[("x1.toml", X1)]);
    connected(&mut w, "X1-NB");
    press(&mut w.h, "Read: Firmware");
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
    assert!(w.sent().is_empty());
    press(&mut w.h, "Run anyway");
    wait(&mut w.h, "the run", |a| a.running.is_none() && logged(a, "finished"));
    let steps: Vec<String> = playbook::dragino_1nce().steps.into_iter().map(|s| s.cmd).collect();
    assert_eq!(w.sent(), steps);
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
    assert_eq!(w.sent().len(), 2);
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
    assert!(w.sent().iter().any(|s| s == "ATZ"));
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

/// Renders the window in both themes to PNGs under `$AIRCORD_SHOTS` (default: `aircord-guided` in the system temp dir), at the smallest window size.
// ---------- the guided view ----------

#[test]
fn the_guided_view_reads_the_settings_and_shows_them_as_a_form() {
    let mut w = lay(s31(), &[]);
    assert!(has(&w, "Noch keine Geräte"));
    lay_connected(&mut w, S31_NAME);
    assert_eq!(w.link.sent(), ["AT+CFG"], "the settings are read without being asked");
    assert_eq!(w.h.state().template.as_deref(), Some("Dragino S31-NB"), "by the model: the name is an IMEI");
    assert!(has(&w, "S31-NB") && has(&w, "v1.4.0"), "the head line names model and firmware");
    assert!(has(&w, "Sendeintervall"));
    assert!(has(&w, "Sendeintervall: 15 Minuten"), "900 seconds, in words");
    assert!(has(&w, "Keine Änderungen"));
    assert!(!has(&w, "Gerätepasswort ändern"), "expert settings stay folded away");
    press(&mut w.h, "Weitere Einstellungen");
    assert!(has(&w, "Gerätepasswort ändern"));
    assert!(!w.h.state().log_text().contains(S31_PASSWORD), "{}", w.h.state().log_text());
}

#[test]
fn a_change_is_counted_shown_before_and_after_and_transferred() {
    let mut w = lay(s31(), &[]);
    lay_connected(&mut w, S31_NAME);
    press(&mut w.h, "Sendeintervall: 15 Minuten");
    press(&mut w.h, "1 Stunde");
    assert!(has(&w, "1 Änderung") && has(&w, "geändert: Sendeintervall"));
    // A setting that only works after a restart, typed.
    w.h.get_by_label("Server-Adresse und Port: Eingabe").click();
    run(&mut w.h);
    w.h.get_by_label("Server-Adresse und Port: Eingabe").type_text("x");
    run(&mut w.h);
    assert!(has(&w, "2 Änderungen"));
    press(&mut w.h, "Zurücksetzen: Server-Adresse und Port");
    assert!(has(&w, "1 Änderung"));
    w.h.state_mut().guided.form.as_mut().unwrap().apply(Edit::Set("SERVADDR".into(), "10.0.0.9,1883".into()));
    press(&mut w.h, "Übertragen");
    assert!(w.sent().is_empty(), "nothing goes out before the yes");
    assert!(has(&w, "Vorher") && has(&w, "Nachher") && has(&w, "15 Minuten") && has(&w, "1 Stunde"));
    assert!(has_part(&w, "Das Gerät kann danach das Netz verlieren"), "the server address can cost the uplink");
    press(&mut w.h, "Jetzt übertragen");
    wait(&mut w.h, "the result", |a| matches!(a.transfer.as_ref().map(|t| &t.stage), Some(Stage::Done(_))));
    assert_eq!(w.sent(), ["AT+SERVADDR=10.0.0.9,1883", "AT+TDC=3600", "AT+CFG"]);
    assert!(has(&w, "Sendeintervall: übernommen") && has(&w, "Server-Adresse und Port: übernommen"));
    assert!(has(&w, "Neu starten, damit alle Änderungen wirken?"));
    assert!(!w.sent().contains(&"ATZ".to_string()), "a restart is offered, never sent by itself");
    press(&mut w.h, "Ja, neu starten");
    wait(&mut w.h, "the restart", |a| a.conn == Conn::Idle);
    assert_eq!(w.sent().last().map(String::as_str), Some("ATZ"));
    assert!(w.h.state().guided.notice.text.contains("startet neu"), "{}", w.h.state().guided.notice.text);
    assert!(!w.h.state().log_text().contains(S31_PASSWORD));
}

#[test]
fn later_leaves_the_device_running_with_the_form_up_to_date() {
    let mut w = lay(s31(), &[]);
    lay_connected(&mut w, S31_NAME);
    w.h.state_mut().guided.form.as_mut().unwrap().apply(Edit::Set("SERVADDR".into(), "10.0.0.9,1883".into()));
    press(&mut w.h, "Übertragen");
    press(&mut w.h, "Jetzt übertragen");
    wait(&mut w.h, "the result", |a| matches!(a.transfer.as_ref().map(|t| &t.stage), Some(Stage::Done(_))));
    press(&mut w.h, "Später");
    assert!(w.h.state().transfer.is_none() && w.h.state().connected());
    assert!(!w.sent().contains(&"ATZ".to_string()));
    assert!(has(&w, "Keine Änderungen"), "what arrived is the device's value now");
    assert_eq!(w.h.state().guided.form.as_ref().unwrap().cfg.get("SERVADDR"), Some("10.0.0.9,1883"));
}

#[test]
fn a_refused_value_is_marked_and_stays_a_change() {
    let link = s31();
    link.reject("TDC", "AT_PARAM_ERROR");
    let mut w = lay(link, &[]);
    lay_connected(&mut w, S31_NAME);
    press(&mut w.h, "Sendeintervall: 15 Minuten");
    press(&mut w.h, "1 Stunde");
    press(&mut w.h, "Übertragen");
    press(&mut w.h, "Jetzt übertragen");
    wait(&mut w.h, "the result", |a| matches!(a.transfer.as_ref().map(|t| &t.stage), Some(Stage::Done(_))));
    assert!(has(&w, "Sendeintervall: Das Gerät hat diesen Wert abgelehnt."), "the refusal is named");
    assert!(!has_part(&w, "AT_") && !has_part(&w, "AT+"), "and the device's code stays out of the lay view");
    assert!(logged(w.h.state(), "AT_PARAM_ERROR"), "the expert's terminal keeps it");
    assert!(!has(&w, "Neu starten, damit alle Änderungen wirken?"));
    press(&mut w.h, "Schließen");
    assert!(has(&w, "1 Änderung"), "the entry stays, to be corrected");
    assert_eq!(w.link.cfg_value("TDC").as_deref(), Some("900"));
}

#[test]
fn a_wrong_input_is_explained_and_blocks_the_transfer() {
    let mut w = lay(s31(), &[]);
    lay_connected(&mut w, S31_NAME);
    press(&mut w.h, "Sendeintervall: 15 Minuten");
    press(&mut w.h, "Eigener Wert…");
    assert!(has(&w, "Sendeintervall: Eingabe"), "the dropdown gave way to a field");
    w.h.state_mut().guided.form.as_mut().unwrap().apply(Edit::Set("TDC".into(), "bald".into()));
    run(&mut w.h);
    assert!(has(&w, "1 Änderung") && has_part(&w, "rot markierten"));
    assert!(has(&w, "Bitte eine ganze Zahl eingeben."), "the German reason from the settings model");
    press(&mut w.h, "Übertragen");
    assert!(w.h.state().transfer.is_none(), "the button is off");
    press(&mut w.h, "Alle zurücksetzen");
    assert!(has(&w, "Keine Änderungen") && has(&w, "Sendeintervall: 15 Minuten"));
}

#[test]
fn a_new_password_never_shows_in_clear() {
    let mut w = lay(s31(), &[]);
    lay_connected(&mut w, S31_NAME);
    press(&mut w.h, "Weitere Einstellungen");
    assert!(!has(&w, "Löschen: Gerätepasswort ändern"), "the device password can't be emptied");
    w.h.state_mut().guided.form.as_mut().unwrap().apply(Edit::Set("PWORD".into(), "wolken".into()));
    run(&mut w.h);
    assert!(has(&w, "1 Änderung"));
    press(&mut w.h, "Übertragen");
    assert!(has(&w, "verdeckt") && has(&w, "neuer Wert (verdeckt)"));
    assert!(has_part(&w, "lässt sich nicht rückgängig machen"));
    assert!(!has_part(&w, "wolken") && !has_part(&w, S31_PASSWORD));
    press(&mut w.h, "Jetzt übertragen");
    wait(&mut w.h, "the result", |a| matches!(a.transfer.as_ref().map(|t| &t.stage), Some(Stage::Done(_))));
    assert_eq!(w.link.cfg_value("PWORD").as_deref(), Some("wolken"), "the device got it");
    assert!(has(&w, "Gerätepasswort ändern: übernommen"));
    let text = w.h.state().log_text();
    assert!(text.contains(">> ***************"), "the link masks the whole command: {text}");
    assert!(!text.contains("wolken") && !text.contains(S31_PASSWORD), "{text}");
    press(&mut w.h, "Schließen");
    press(&mut w.h, "Experte");
    assert!(!has_part(&w, "wolken") && !has_part(&w, S31_PASSWORD), "nor in the terminal on screen");
}

#[test]
fn the_expert_tab_keeps_the_terminal_and_its_tools() {
    let mut w = lay(s31(), &[]);
    lay_connected(&mut w, S31_NAME);
    assert!(!has(&w, "Send") && !has(&w, "Playbooks"));
    press(&mut w.h, "Experte");
    for label in ["Terminal", "Send", "Playbooks", "Sessions", "Firmware", "Send: Neu starten", "Disconnect"] {
        assert!(has(&w, label), "{label}");
    }
    w.h.state_mut().cmd = "AT+TDC=?".into();
    press(&mut w.h, "Send");
    assert_eq!(w.sent(), ["AT+TDC=?"]);
    press(&mut w.h, "Einstellungen");
    assert!(has(&w, "Sendeintervall: 15 Minuten"), "the form is still there");
}

#[test]
fn a_model_without_a_template_gets_a_hint_and_the_way_to_the_expert_view() {
    let mut w = lay(FakeLink::configured("aa", "Sensor 7", "AT+MODEL=ZZ9-XX,v0.1.0\nAT+TDC=60\nOK"), &[]);
    lay_connected(&mut w, "Sensor 7");
    assert!(has(&w, "ZZ9-XX") && has_part(&w, "noch keine geführten Einstellungen"));
    assert!(!has(&w, "Übertragen"));
    press(&mut w.h, "Experten-Ansicht öffnen");
    assert_eq!(w.h.state().view, View::Expert);
    assert!(has(&w, "Terminal"));
}

#[test]
fn a_device_that_keeps_its_settings_back_is_asked_for_the_password() {
    // Takes every command and stays silent when asked for its settings.
    let link = FakeLink::console("aa", "Sensor 7");
    link.reply("AT+CFG", &[]);
    let mut w = lay(link, &[]);
    lay_connected(&mut w, "Sensor 7");
    assert!(has(&w, "Passwort: Eingabe") && has_part(&w, "Etikett"));
    press(&mut w.h, "Anmelden");
    assert!(w.sent().is_empty() && w.h.state().guided.notice.text.contains("Passwort eingeben"));
    w.h.state_mut().pin = "349011".into();
    press(&mut w.h, "Anmelden");
    wait(&mut w.h, "the second read", |a| a.guided.password_tried && matches!(a.guided.read, Read::Failed(_)));
    assert_eq!(w.sent(), ["******", "AT+CFG"], "the password goes out masked, then the settings are asked for again");
    assert!(has_part(&w, "nicht angenommen"));
    assert!(!w.h.state().log_text().contains("349011"));
}

#[test]
fn a_template_s_secret_is_masked_in_what_the_terminal_already_holds() {
    let mut w = world_with(FakeLink::console("aa", "Sensor 7"), &[("x1.toml", X1)]);
    connected(&mut w, "Sensor 7");
    let app = w.h.state_mut();
    for line in ["<< AT+TOKEN=tok-abc", ">> at+token=tok-neu", "-- note: TOKEN=frei --"] {
        app.log_line(line);
    }
    app.pick_template(Some("Prüfgerät X1".into()));
    assert!(logged(app, "<< AT+TOKEN=******") && logged(app, ">> at+token=******"), "typed in either case");
    assert!(logged(app, "-- note: TOKEN=frei --"), "a line without a command is not touched");
    assert!(!app.log_text().contains("tok-"));
}

#[test]
fn the_status_line_and_an_error_mask_what_the_terminal_masks() {
    let mut w = world_with(FakeLink::configured("aa", "X1-NB", X1_CFG), &[("x1.toml", X1)]);
    connected(&mut w, "X1-NB");
    let app = w.h.state_mut();
    app.set_status("playbook p — step 1/2: AT+TOKEN=tok-neu");
    app.fail("no reply to AT+PWORD=wolken");
    assert_eq!(app.status, "playbook p — step 1/2: AT+TOKEN=******");
    assert_eq!(app.alert.text, "no reply to AT+PWORD=******");
    assert!(!app.log_text().contains("wolken"));
}

#[test]
fn a_device_card_names_its_signal_and_whether_it_is_connected() {
    let link = s31();
    link.add_nearby("bb", "Sensor 9", -80);
    let mut w = lay(link, &[]);
    press(&mut w.h, "Suchen");
    press(&mut w.h, "Stopp");
    assert!(has(&w, &format!("{S31_NAME}, Signal sehr gut")) && has(&w, "Sensor 9, Signal mittel"));
    press_card(&mut w.h, S31_NAME);
    press(&mut w.h, "Verbinden");
    settled(&mut w);
    run(&mut w.h);
    assert!(has(&w, &format!("{S31_NAME}, verbunden")) && has(&w, "Sensor 9, Signal mittel"));
}

#[test]
fn a_restart_without_a_link_says_so_and_sends_nothing() {
    let mut w = lay(s31(), &[]);
    lay_connected(&mut w, S31_NAME);
    w.link.drop_link();
    wait(&mut w.h, "the drop", |a| a.conn == Conn::Idle);
    w.h.state_mut().restart_device("ATZ".into());
    run(&mut w.h);
    assert!(w.sent().is_empty());
    assert!(has_part(&w, "Keine Verbindung") && !has_part(&w, "startet neu"));
}

#[test]
fn an_answer_that_is_no_settings_does_not_ask_for_the_password() {
    for reply in [&["AT_ERROR"][..], &["AT+TDC=900", "OK"], &["OK"]] {
        let link = FakeLink::console("aa", "Sensor 7");
        link.reply("AT+CFG", reply);
        let mut w = lay(link, &[]);
        lay_connected(&mut w, "Sensor 7");
        assert!(matches!(w.h.state().guided.read, Read::Failed(_)), "{reply:?}");
        assert!(!has(&w, "Passwort: Eingabe") && has(&w, "Erneut lesen"), "{reply:?}");
    }
}

#[test]
fn the_password_typed_before_connecting_unlocks_and_the_status_card_fills() {
    let link = s31();
    link.reply("349011", &S31_BOOT.lines().collect::<Vec<_>>());
    let mut w = lay(link, &[]);
    w.h.state_mut().pin = "349011".into();
    lay_connected(&mut w, S31_NAME);
    assert_eq!(w.link.sent(), ["******", "AT+CFG"]);
    wait(&mut w.h, "the start lines", |a| a.guided.status.signal.is_some());
    for label in ["Passwort angenommen", "SIM-Karte erkannt", "Kein Netz · Band 8,20", "Zugangspunkt gesetzt"] {
        assert!(has(&w, label), "{label}");
    }
    assert!(!has(&w, NO_NETWORK_HINT), "not at the first report");
    // Minutes later.
    w.h.state_mut().timing.no_network_hint = Duration::ZERO;
    run(&mut w.h);
    assert!(w.h.state().no_network_for_long());
    assert!(has(&w, NO_NETWORK_HINT));
    w.h.state_mut().feed_status("[30100]Signal Strength:17");
    run(&mut w.h);
    assert!(has(&w, "Netz gut · Band 8,20") && !has(&w, NO_NETWORK_HINT));
}

#[test]
fn a_secret_of_a_template_found_by_the_model_is_masked_from_its_first_line() {
    // Nothing in the name picks the template: it is known only once the dump names the model.
    let mut w = lay(FakeLink::configured("aa", S31_NAME, X1_CFG), &[("x1.toml", X1)]);
    lay_connected(&mut w, S31_NAME);
    assert_eq!(w.h.state().template.as_deref(), Some("Prüfgerät X1"));
    let text = w.h.state().log_text();
    assert!(text.contains("<< AT+TOKEN=******") && !text.contains("tok-abc"), "{text}");
    assert!(has(&w, "Zugangsschlüssel") && !has_part(&w, "tok-abc"));
}

#[test]
fn a_template_picked_by_hand_survives_the_next_read() {
    // The name says nothing; the model says S31.
    let link = FakeLink::configured("aa", "Sensor 7", S31_CFG);
    link.reply("AT+FW", &["X1-NB firmware v1.4.2", "OK"]);
    let mut w = world_with(link, &[("x1.toml", X1)]);
    connected(&mut w, "Sensor 7");
    assert_eq!(w.h.state().template.as_deref(), Some("Dragino S31-NB"), "nobody chose: the model decides");
    w.h.state_mut().pick_template(Some("Prüfgerät X1".into()));
    run(&mut w.h);
    press(&mut w.h, "Read: Firmware");
    wait(&mut w.h, "the value", |a| a.values.values().any(|v| v == "1.4.2"));
    w.h.state_mut().read_again();
    let link = Arc::clone(&w.link);
    wait(&mut w.h, "the second read", |a| a.guided.read == Read::Ready && link.sent().iter().filter(|c| *c == "AT+CFG").count() == 2);
    let app = w.h.state();
    assert_eq!(app.template.as_deref(), Some("Prüfgerät X1"), "the model does not take the panel back");
    assert!(app.values.values().any(|v| v == "1.4.2"), "and what the panel read stays");
    assert_eq!(app.guided.form.as_ref().map(|f| f.template.name.as_str()), Some("Dragino S31-NB"), "the form is the model's");
    assert!(!logged(app, S31_PASSWORD), "the model's secrets stay masked under another template");
}

#[test]
fn a_session_s_template_is_not_replaced_by_the_model_s() {
    let mut w = world_with(FakeLink::configured("aa", "Sensor 7", S31_CFG), &[("x1.toml", X1)]);
    w.h.state_mut().want_template = Some("Prüfgerät X1".into());
    connected(&mut w, "Sensor 7");
    let app = w.h.state();
    assert_eq!(app.template.as_deref(), Some("Prüfgerät X1"));
    assert_eq!(app.guided.form.as_ref().map(|f| f.template.name.as_str()), Some("Dragino S31-NB"));
    assert!(!logged(app, S31_PASSWORD));
}

/// A device that takes its time with the dump, so the read after a connect is still under way.
fn slow() -> FakeLink {
    slow_by(Duration::from_millis(250))
}

fn slow_by(late: Duration) -> FakeLink {
    let link = FakeLink::console("aa", "Sensor 7");
    link.reply_late("AT+CFG", late, &["AT+MODEL=ZZ9-XX,v0.1.0", "AT+TDC=60", "OK"]);
    link
}

/// Connects in the expert view and returns while the settings are still being read.
fn reading(w: &mut World, name: &str) {
    press(&mut w.h, "Scan");
    press(&mut w.h, "Stop");
    press(&mut w.h, name);
    press(&mut w.h, "Connect");
    wait(&mut w.h, "the connection", |a| matches!(a.conn, Conn::Connected { .. }));
    let link = Arc::clone(&w.link);
    wait(&mut w.h, "the read to start", |_| link.sent().contains(&"AT+CFG".to_string()));
    assert_eq!(w.h.state().guided.read, Read::Reading);
}

#[test]
fn a_command_typed_during_the_read_goes_out_and_ends_the_read() {
    let link = slow();
    link.reply("AT+VER", &["v1.4.2", "OK"]);
    let mut w = world(link);
    reading(&mut w, "Sensor 7");
    w.h.state_mut().cmd = "AT+VER".into();
    press(&mut w.h, "Send");
    assert_eq!(w.link.sent(), ["AT+CFG", "AT+VER"], "the command is not held back");
    assert_eq!(w.h.state().guided.read, Read::Idle, "the read counts as stopped");
    wait(&mut w.h, "the reply", |a| logged(a, "<< v1.4.2"));
    std::thread::sleep(Duration::from_millis(700));
    run(&mut w.h);
    let app = w.h.state();
    assert_eq!(app.guided.read, Read::Idle, "the reply was not taken for the dump, nor the late dump for a result");
    assert!(app.guided.form.is_none() && app.guided.model.is_none());
    assert!(logged(app, "reading the settings stopped"));
}

#[test]
fn a_panel_action_during_the_read_ends_it_too() {
    let link = FakeLink::console("aa", "X1-NB");
    link.reply_late("AT+CFG", Duration::from_millis(250), &["AT+MODEL=X1-NB,v1.0.0", "AT+TDC=900", "OK"]);
    link.reply("AT+FW", &["X1-NB firmware v1.4.2", "OK"]);
    let mut w = world_with(link, &[("x1.toml", X1)]);
    reading(&mut w, "X1-NB");
    press(&mut w.h, "Read: Firmware");
    assert_eq!(w.h.state().guided.read, Read::Idle);
    wait(&mut w.h, "the value", |a| a.values.values().any(|v| v == "1.4.2"));
}

#[test]
fn a_playbook_started_in_the_window_ends_a_read_under_way() {
    // Three presses lie between the connect and the run: the dump must not arrive before them on a busy machine.
    // The playbook asks for the dump itself, so its own wait has to outlast the slow answer.
    let mut w = world(slow_by(Duration::from_millis(600)));
    w.h.state_mut().timing.reply_wait = Duration::from_secs(1);
    reading(&mut w, "Sensor 7");
    press(&mut w.h, "Playbooks");
    press(&mut w.h, "Run");
    press(&mut w.h, "Run anyway");
    assert_eq!(w.h.state().guided.read, Read::Idle);
    assert!(logged(w.h.state(), "reading the settings stopped"));
    wait(&mut w.h, "the run", |a| a.running.is_none() && logged(a, "finished"));
    let steps: Vec<String> = playbook::dragino_1nce().steps.into_iter().map(|s| s.cmd).collect();
    assert_eq!(w.link.sent()[1..], steps, "after the read's AT+CFG, the steps and nothing else");
    assert_eq!(w.h.state().guided.read, Read::Idle);
}

#[test]
fn a_pin_is_not_sent_to_the_device_connected_next() {
    let mut w = world(FakeLink::console("aa", "Sensor 7"));
    w.h.state_mut().timing.settle = Duration::from_millis(300);
    w.h.state_mut().pin = "349011".into();
    press(&mut w.h, "Scan");
    press(&mut w.h, "Stop");
    press(&mut w.h, "Sensor 7");
    press(&mut w.h, "Connect");
    wait(&mut w.h, "the connection", |a| matches!(a.conn, Conn::Connected { .. }));
    // Let go and connect again without a PIN, while the first connect still waits to send its own.
    press(&mut w.h, "Disconnect");
    w.h.state_mut().pin.clear();
    press(&mut w.h, "Connect");
    settled(&mut w);
    std::thread::sleep(Duration::from_millis(400));
    run(&mut w.h);
    assert_eq!(w.link.sent(), ["AT+CFG"], "the first connect's PIN never went out");
}

#[test]
fn nothing_is_transferred_over_settings_that_are_not_fresh() {
    let mut w = lay(s31(), &[]);
    lay_connected(&mut w, S31_NAME);
    press(&mut w.h, "Sendeintervall: 15 Minuten");
    press(&mut w.h, "1 Stunde");
    w.link.reply("AT+CFG", &["AT_ERROR"]);
    press(&mut w.h, "Einstellungen neu lesen");
    wait(&mut w.h, "the failed read", |a| matches!(a.guided.read, Read::Failed(_)));
    run(&mut w.h);
    assert!(has(&w, "1 Änderung") && has_part(&w, "zuerst die Einstellungen neu lesen"));
    assert!(has(&w, "Das Gerät hat die Abfrage abgelehnt. Bitte erneut versuchen."));
    assert!(!has_part(&w, "AT_") && !has_part(&w, "AT+"), "no command names in the lay view");
    press(&mut w.h, "Übertragen");
    assert!(w.h.state().transfer.is_none(), "the button is off");
    w.h.state_mut().review_transfer();
    assert!(w.h.state().transfer.is_none(), "and the dialog does not open another way");
}

#[test]
fn a_playbook_takes_the_link_from_a_read_under_way() {
    let link = FakeLink::console("aa", "Sensor 7");
    // A device that takes its time with the dump.
    link.reply_late("AT+CFG", Duration::from_millis(250), &["AT+MODEL=ZZ9-XX,v0.1.0", "AT+TDC=60", "OK"]);
    let mut w = world(link);
    press(&mut w.h, "Scan");
    press(&mut w.h, "Stop");
    press(&mut w.h, "Sensor 7");
    press(&mut w.h, "Connect");
    wait(&mut w.h, "the connection", |a| matches!(a.conn, Conn::Connected { .. }));
    assert_eq!(w.h.state().guided.read, Read::Reading);
    w.h.state_mut().interrupt_read();
    assert_eq!(w.h.state().guided.read, Read::Idle);
    std::thread::sleep(Duration::from_millis(700));
    run(&mut w.h);
    assert_eq!(w.h.state().guided.read, Read::Idle, "the stopped read's late result is dropped");
    press(&mut w.h, "Einstellungen");
    press(&mut w.h, "Jetzt lesen");
    wait(&mut w.h, "the read", |a| a.guided.read == Read::Ready);
    assert!(has(&w, "ZZ9-XX"));
}

#[test]
fn a_dropped_link_empties_the_guided_view_and_says_so() {
    let mut w = lay(s31(), &[]);
    lay_connected(&mut w, S31_NAME);
    press(&mut w.h, "Sendeintervall: 15 Minuten");
    press(&mut w.h, "1 Stunde");
    w.link.drop_link();
    wait(&mut w.h, "the drop", |a| a.conn == Conn::Idle);
    run(&mut w.h);
    assert!(w.h.state().guided.form.is_none());
    assert!(has_part(&w, "Verbindung zum Gerät ist abgebrochen") && !has(&w, "Übertragen"));
}

#[cfg(feature = "screenshot")]
#[test]
#[ignore = "needs a GPU or a software renderer; run with --ignored"]
fn screenshot() {
    let out =
        std::env::var_os("AIRCORD_SHOTS").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("aircord-guided"));
    std::fs::create_dir_all(&out).unwrap();
    for (theme, name) in [(egui::Theme::Dark, "dark"), (egui::Theme::Light, "light")] {
        let link = FakeLink::configured("6c1a3b52-8f0e-4d55-9a21-0e7d2c4b9f10", S31_NAME, S31_CFG);
        link.add_nearby("2f9b7c1e-1d3a-4c8b-8e6f-5a4b3c2d1e0f", "866207050000002", -71);
        link.add_nearby("9a8b7c6d-5e4f-4a3b-2c1d-0e9f8a7b6c5d", "Sensor Keller", -88);
        let mut boot: Vec<&str> = S31_BOOT.lines().collect();
        boot.push("[30100]Signal Strength:21");
        link.reply("349011", &boot);
        let dir = tempfile::tempdir().unwrap();
        let link = Arc::new(link);
        let inbox = Inbox::default();
        link.set_events(Arc::new(inbox.outbox()));
        let mut app = App::new(Arc::clone(&link) as Arc<dyn Link>, Stores::at(dir.path()), inbox);
        app.timing.settle = Duration::from_millis(10);
        app.timing.read_wait = Duration::from_millis(300);
        app.pin = "349011".into();
        // The smallest window the app allows.
        let mut h = Harness::builder()
            .with_size([900.0, 560.0])
            .with_max_steps(16)
            .with_step_dt(0.1)
            .wgpu()
            .build_ui_state(|ui, app: &mut App| app.show(ui), app);
        h.ctx.set_theme(theme);
        run(&mut h);
        let shot = |h: &mut Harness<App>, what: &str| {
            run(h);
            h.render().unwrap().save(out.join(format!("aircord-{what}-{name}.png"))).unwrap();
        };
        press(&mut h, "Suchen");
        press(&mut h, "Stopp");
        press_card(&mut h, S31_NAME);
        shot(&mut h, "devices");
        press(&mut h, "Verbinden");
        wait(&mut h, "the settings", |a| a.guided.read == Read::Ready && a.guided.status.signal.is_some_and(|s| s.has_network()));
        press(&mut h, "Sendeintervall: 15 Minuten");
        shot(&mut h, "dropdown");
        press(&mut h, "1 Stunde");
        h.state_mut().guided.form.as_mut().unwrap().apply(Edit::Set("SERVADDR".into(), "10.0.0.9,1883".into()));
        shot(&mut h, "guided");
        press(&mut h, "Übertragen");
        shot(&mut h, "review");
        press(&mut h, "Jetzt übertragen");
        wait(&mut h, "the result", |a| matches!(a.transfer.as_ref().map(|t| &t.stage), Some(Stage::Done(_))));
        shot(&mut h, "result");
        press(&mut h, "Später");
        press(&mut h, "Weitere Einstellungen");
        shot(&mut h, "more");
        press(&mut h, "Experte");
        shot(&mut h, "expert");
    }
}
