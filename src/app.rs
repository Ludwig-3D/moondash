use crate::icons;
use crate::print_files::{self, array};
use crate::{
    ColorSwatch, FilamentSensor, I18n, Lane, Moondash, Notification, Palette, PrintTool, Row,
    WifiEntry, config, input_idle, locale,
    moonraker::{self, Call, Event},
    network, theme, version, wayland_power,
};
use serde_json::{Map, Value, json};
use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime},
};
use tokio::{
    runtime::Handle,
    sync::{mpsc as async_mpsc, watch},
};

const DEFAULT_CONFIG: &str = "[websocket]\nip: 127.0.0.1:7125\n\n[styling]\ndarkmode: true\n\n[system]\nlanguage: en\nuse_idle_timeout: true\nidle_timeout: 900\nidle_unlock: 500\n";
const NOTIFICATION_DISMISSALS: &str = "notification_dismissals";

pub struct App {
    _timer: Timer,
    _state: Rc<RefCell<State>>,
}

#[derive(Default)]
struct Dialog {
    action: String,
    argument: String,
}

enum Background {
    Reply(String, Result<Value, String>),
    Image(String, Result<Vec<u8>, String>),
    Sleeping(bool),
}

struct State {
    config_path: PathBuf,
    config_modified: Option<SystemTime>,
    config: Value,
    endpoint: watch::Sender<String>,
    calls: async_mpsc::Sender<Call>,
    runtime: Handle,
    background: mpsc::Sender<Background>,
    http: reqwest::Client,
    idle_config: Arc<Mutex<Value>>,
    manual_sleep: Arc<AtomicBool>,
    online: bool,
    status: Map<String, Value>,
    moonraker_warnings: Value,
    dismissed_notifications: HashSet<String>,
    page: String,
    tab: String,
    folder: String,
    file_listing: Value,
    file_history: Value,
    file_history_checked: Option<Instant>,
    browsing_folder: String,
    browse_entries: Vec<String>,
    destination_folder: String,
    print_mapping: Option<String>,
    filter: String,
    file_sort: u8,
    file_details: HashMap<String, Value>,
    file_previews: HashMap<String, Image>,
    requested_files: HashSet<String>,
    motion_busy: bool,
    afc_unit: String,
    tool_view: String,
    queue: Value,
    network: Value,
    updates: Value,
    host: Value,
    dialog: Dialog,
    edited_speed_percent: Option<i32>,
    initial_speed_acceleration: Option<f64>,
    filename: String,
    metadata: Value,
    jog_step: f64,
    config_checked: Instant,
    refresh_at: Instant,
    webcam: Option<String>,
    webcam_image_pending: bool,
}

pub fn start(
    ui: &Moondash,
    path: PathBuf,
    runtime: Handle,
) -> Result<App, Box<dyn std::error::Error>> {
    let (events_tx, events_rx) = mpsc::channel();
    let (background, background_rx) = mpsc::channel();
    let (calls, calls_rx) = async_mpsc::channel(32);
    let (endpoint, endpoint_rx) = watch::channel(String::new());
    let idle_config = Arc::new(Mutex::new(Value::Null));
    let manual_sleep = Arc::new(AtomicBool::new(false));

    let state = Rc::new(RefCell::new(State {
        config_path: path,
        config_modified: None,
        config: Value::Null,
        endpoint,
        calls,
        runtime: runtime.clone(),
        background: background.clone(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()?,
        idle_config: idle_config.clone(),
        manual_sleep: manual_sleep.clone(),
        online: false,
        status: Map::new(),
        moonraker_warnings: Value::Null,
        dismissed_notifications: HashSet::new(),
        page: "home".into(),
        tab: String::new(),
        folder: String::new(),
        file_listing: Value::Null,
        file_history: Value::Null,
        file_history_checked: None,
        browsing_folder: String::new(),
        browse_entries: Vec::new(),
        destination_folder: String::new(),
        print_mapping: None,
        filter: String::new(),
        file_sort: 0,
        file_details: HashMap::new(),
        file_previews: HashMap::new(),
        requested_files: HashSet::new(),
        motion_busy: false,
        afc_unit: String::new(),
        tool_view: String::new(),
        queue: Value::Null,
        network: Value::Null,
        updates: Value::Null,
        host: Value::Null,
        dialog: Dialog::default(),
        edited_speed_percent: None,
        initial_speed_acceleration: None,
        filename: String::new(),
        metadata: Value::Null,
        jog_step: 10.,
        config_checked: Instant::now(),
        refresh_at: Instant::now(),
        webcam: None,
        webcam_image_pending: false,
    }));

    ui.global::<I18n>()
        .on_lookup(|key, german| locale::text(key.as_str(), german).into());
    state.borrow_mut().load_config(ui, true);
    runtime.spawn(moonraker::run(endpoint_rx, calls_rx, events_tx));
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        start_display_watcher(idle_config, manual_sleep, background);
    }

    let weak = ui.as_weak();
    let shared = state.clone();
    ui.on_navigate(move |page, tab| {
        if let Some(ui) = weak.upgrade() {
            shared
                .borrow_mut()
                .navigate(&ui, page.as_str(), tab.as_str());
        }
    });
    let weak = ui.as_weak();
    let shared = state.clone();
    ui.on_action(move |action, argument| {
        if let Some(ui) = weak.upgrade() {
            shared
                .borrow_mut()
                .action(&ui, action.as_str(), argument.as_str());
        }
    });
    let weak = ui.as_weak();
    let shared = state.clone();
    ui.on_dialog_submit(move |value, queue| {
        if let Some(ui) = weak.upgrade() {
            shared.borrow_mut().submit(&ui, value.as_str(), queue);
        }
    });
    let weak = ui.as_weak();
    let shared = state.clone();
    ui.on_adjust_speed(move |step| {
        if let Some(ui) = weak.upgrade() {
            let percent = ui.get_speed_target().saturating_add(step).max(1);
            shared.borrow_mut().edited_speed_percent = Some(percent);
            ui.set_speed_target(percent);
        }
    });
    ui.on_format_speed_percent(|percent| format_speed_percent(percent).into());
    let weak = ui.as_weak();
    ui.on_dialog_close(move || {
        if let Some(ui) = weak.upgrade()
            && !ui.get_file_busy()
        {
            ui.set_keyboard_target("".into());
            ui.set_dialog_mode("".into());
            ui.set_folder_picker(false);
            ui.set_print_lane_picker(false);
        }
    });
    let weak = ui.as_weak();
    ui.on_erase_key(move || {
        if let Some(ui) = weak.upgrade() {
            let mut value = ui.get_dialog_value().to_string();
            value.pop();
            ui.set_dialog_value(value.into());
        }
    });

    let weak = ui.as_weak();
    let shared = state.clone();
    ui.on_filter_edited(move |value| {
        if let Some(ui) = weak.upgrade() {
            shared.borrow_mut().filter_files(&ui, value.as_str());
        }
    });
    let weak = ui.as_weak();
    let shared = state.clone();
    ui.on_keyboard_erase(move |target| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        match target.as_str() {
            "wifi-ssid" => {
                ui.set_network_ssid(without_last(ui.get_network_ssid()).into());
                return;
            }
            "wifi-password" => {
                ui.set_network_password(without_last(ui.get_network_password()).into());
                return;
            }
            _ => {}
        }
        let mut state = shared.borrow_mut();
        match target.as_str() {
            "filter" => state.filter_files(&ui, &without_last(ui.get_filter())),
            "move-name" => ui.set_move_name(without_last(ui.get_move_name()).into()),
            "folder-name" => ui.set_folder_name(without_last(ui.get_folder_name()).into()),
            _ => return,
        }
        state.validate_picker(&ui);
    });
    ui.on_remove_acceleration_digit(|value| without_last(value).into());
    let weak = ui.as_weak();
    let shared = state.clone();
    ui.on_picker_edited(move || {
        if let Some(ui) = weak.upgrade() {
            shared.borrow().validate_picker(&ui);
        }
    });

    let timer = Timer::default();
    let weak = ui.as_weak();
    let shared = state.clone();
    timer.start(TimerMode::Repeated, Duration::from_millis(75), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let mut state = shared.borrow_mut();
        let mut latest_status = None;
        for event in events_rx.try_iter().take(64) {
            match event {
                Event::Status(status) => latest_status = Some(status),
                event => {
                    if let Some(status) = latest_status.take() {
                        state.set_status(&ui, status);
                    }
                    state.event(&ui, event);
                }
            }
        }
        if let Some(status) = latest_status {
            state.set_status(&ui, status);
        }
        for event in background_rx.try_iter().take(32) {
            state.background_event(&ui, event);
        }
        state.tick(&ui);
    });
    Ok(App {
        _timer: timer,
        _state: state,
    })
}

impl State {
    fn load_config(&mut self, ui: &Moondash, force: bool) {
        let modified = fs::metadata(&self.config_path)
            .and_then(|metadata| metadata.modified())
            .ok();
        if !force && self.config_modified == modified {
            return;
        }
        self.config_modified = modified;

        let text = match fs::read_to_string(&self.config_path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => DEFAULT_CONFIG.to_owned(),
            Err(error) => {
                ui.set_message(format!("{}: {error}", self.config_path.display()).into());
                DEFAULT_CONFIG.to_owned()
            }
        };
        match config::parse(&text) {
            Ok(value) => {
                self.config = value;
                ui.set_config_text(text.into());
            }
            Err(error) => {
                ui.set_message(error.into());
                if force {
                    ui.set_config_text(text.into());
                }
                if self.config.is_null() {
                    self.config = config::default_config();
                }
            }
        }
        if let Ok(mut config) = self.idle_config.lock() {
            *config = self.config["system"].clone();
        }
        let host = self.config["websocket"]["ip"]
            .as_str()
            .unwrap_or("127.0.0.1:7125")
            .to_owned();
        if self.endpoint.send_if_modified(|endpoint| {
            if *endpoint == host {
                return false;
            }
            *endpoint = host;
            true
        }) {
            self.dismissed_notifications.clear();
        }

        let dark = self.config["styling"]["darkmode"].as_bool().unwrap_or(true);
        ui.set_dark(dark);
        let palette = ui.global::<Palette>();
        palette.set_dark(dark);
        let [primary, secondary] = theme::theme_defaults(dark);
        palette.set_accent(config_color(&self.config["styling"]["primary"], primary));
        let language = match self.config["system"]["language"].as_str() {
            Some(language @ ("en" | "de")) => language,
            _ => ["LC_ALL", "LC_MESSAGES", "LANG"]
                .into_iter()
                .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
                .filter(|locale| locale.split(['_', '-', '.', '@']).next() == Some("de"))
                .map_or("en", |_| "de"),
        };
        ui.global::<I18n>().set_language(language.into());
        ui.set_debug(self.config["dev"]["debug"].as_bool().unwrap_or(false));
        if let Some(logo) = theme::custom_logo(&self.config_path) {
            ui.set_logo(logo);
        }
        palette.set_secondary(config_color(
            &self.config["styling"]["secondary"],
            secondary,
        ));
        let profile = self.config["system"]["display_profile"]
            .as_str()
            .or_else(|| self.config["styling"]["display_profile"].as_str())
            .unwrap_or("auto");
        let (rows, cols) = match profile {
            "small" => (2, 3),
            "normal" => (3, 4),
            "highres" => (4, 6),
            _ => (0, 0),
        };
        ui.set_file_rows_config(
            config_int(&self.config["styling"]["printfiles_grid_row"], 8).unwrap_or(rows),
        );
        ui.set_file_columns_config(
            config_int(&self.config["styling"]["printfiles_grid_columns"], 8).unwrap_or(cols),
        );
        self.update_shortcuts(ui);
        self.update_notifications(ui);
        self.render_rows(ui);
    }

    fn tick(&mut self, ui: &Moondash) {
        if self.config_checked.elapsed() >= Duration::from_secs(1) {
            self.config_checked = Instant::now();
            self.load_config(ui, false);
        }
        if self.page == "files" && self.online {
            self.load_visible_file_details(ui);
        }
        if self.refresh_at.elapsed() >= Duration::from_secs(10) {
            self.refresh_at = Instant::now();
            if self.online {
                self.request_moonraker_warnings();
            }
            match (self.page.as_str(), self.tab.as_str()) {
                ("settings", "network") => self.network_action(ui, "network", "", ""),
                ("settings", "host") if self.online => {
                    self.call(ui, "machine.proc_stats", json!({}), "host-stats")
                }
                ("home", _) | ("tune", "queue") if self.online => {
                    self.call(ui, "server.job_queue.status", json!({}), "queue")
                }
                _ => {}
            }
        }
        if self.page == "settings"
            && self.tab == "tools"
            && !self.webcam_image_pending
            && let Some(url) = self.webcam.clone()
        {
            self.webcam_image_pending = true;
            self.fetch_image("webcam".into(), &url);
        }
    }

    fn navigate(&mut self, ui: &Moondash, page: &str, tab: &str) {
        self.page = page.into();
        self.tab = tab.into();
        ui.set_page(page.into());
        ui.set_tab(tab.into());
        if (page, tab) == ("tune", "afc") {
            self.update_afc(ui);
        }
        if (page, tab) != ("settings", "tools") {
            self.webcam = None;
            self.tool_view.clear();
            ui.set_tool_title("".into());
            ui.set_tool_image(Image::default());
        }
        self.refresh(ui);
    }

    fn refresh(&mut self, ui: &Moondash) {
        self.refresh_at = Instant::now();
        match (self.page.as_str(), self.tab.as_str()) {
            ("files", _) if self.online => {
                if self.file_listing.is_null() {
                    ui.set_file_loading(true);
                }
                if !self.try_call(ui, "server.files.list", json!({}), "file-list") {
                    ui.set_file_loading(false);
                }
                if self
                    .file_history_checked
                    .is_none_or(|checked| checked.elapsed() >= Duration::from_secs(30))
                {
                    self.file_history_checked = Some(Instant::now());
                    self.call(
                        ui,
                        "server.history.list",
                        json!({"limit":100,"order":"desc"}),
                        "file-history",
                    );
                }
            }
            ("home", _) | ("tune", "queue") if self.online => {
                self.call(ui, "server.job_queue.status", json!({}), "queue")
            }
            ("settings", "network") => self.network_action(ui, "network", "", ""),
            ("settings", "updates") if self.online => self.call(
                ui,
                "machine.update.status",
                json!({"refresh": false}),
                "updates",
            ),
            ("settings", "host") if self.online => {
                self.call(ui, "machine.system_info", json!({}), "host-info");
                self.call(ui, "machine.proc_stats", json!({}), "host-stats");
            }
            _ => {}
        }
        self.render_rows(ui);
    }

    fn call(&self, ui: &Moondash, method: &'static str, params: Value, tag: impl Into<String>) {
        self.try_call(ui, method, params, tag);
    }

    fn request_moonraker_warnings(&self) {
        let _ = self.calls.try_send(Call::new(
            "server.info",
            json!({}),
            "notification-server-info",
        ));
    }

    fn request_update_notifications(&self) {
        let _ = self.calls.try_send(Call::new(
            "machine.update.status",
            json!({"refresh": false}),
            "notification-updates",
        ));
    }

    fn try_call(
        &self,
        ui: &Moondash,
        method: &'static str,
        params: Value,
        tag: impl Into<String>,
    ) -> bool {
        if !self.online {
            ui.set_message(tr(ui, "moonraker_is_disconnected").into());
        } else if self.calls.try_send(Call::new(method, params, tag)).is_err() {
            ui.set_message(tr(ui, "moonraker_request_queue_is_busy").into());
        } else {
            return true;
        }
        false
    }

    fn motion(&mut self, ui: &Moondash, script: impl Into<String>) {
        if ui.get_printing() || ui.get_paused() {
            return;
        }
        self.submit_control(ui, script);
    }

    fn submit_control(&mut self, ui: &Moondash, script: impl Into<String>) {
        if !ui.get_ready() || self.motion_busy {
            return;
        }
        let call = Call::new(
            "printer.gcode.script",
            json!({"script": script.into()}),
            "motion",
        );
        if self.calls.try_send(call).is_ok() {
            self.motion_busy = true;
            ui.set_motion_busy(true);
        } else {
            ui.set_message(tr(ui, "moonraker_request_queue_is_busy").into());
        }
    }

    fn script(&self, ui: &Moondash, script: impl Into<String>) {
        if ui.get_ready() {
            self.call(
                ui,
                "printer.gcode.script",
                json!({"script": script.into()}),
                "gcode",
            );
        } else {
            ui.set_message(tr(ui, "klipper_is_not_ready").into());
        }
    }

    fn event(&mut self, ui: &Moondash, event: Event) {
        match event {
            Event::Connected => {
                self.online = true;
                self.dismissed_notifications
                    .retain(|id| is_persistent_warning(id));
                ui.set_connected_once(true);
                ui.set_online(true);
                ui.set_connection("".into());
                ui.set_message("".into());
                let _ = self.calls.try_send(Call::new(
                    "server.database.get_item",
                    json!({"namespace": "moondash", "key": NOTIFICATION_DISMISSALS}),
                    "notification-dismissals",
                ));
                self.request_moonraker_warnings();
                self.request_update_notifications();
                self.refresh(ui);
            }
            Event::Disconnected(reason) => {
                ui.set_connection(reason.into());
                if self.online {
                    self.online = false;
                    self.status.clear();
                    self.moonraker_warnings = Value::Null;
                    self.updates = Value::Null;
                    self.file_listing = Value::Null;
                    self.file_history = Value::Null;
                    self.file_history_checked = None;
                    ui.set_file_loading(false);
                    self.queue = Value::Null;
                    self.requested_files.clear();
                    self.file_details.clear();
                    self.file_previews.clear();
                    self.motion_busy = false;
                    ui.set_motion_busy(false);
                    self.update_jobs(ui);
                    self.set_status(ui, Map::new());
                }
                self.online = false;
                if ui.get_file_busy() {
                    ui.set_file_busy(false);
                    let message = tr(ui, "moonraker_disconnected");
                    ui.set_dialog_detail(message.into());
                    ui.set_folder_picker_error(message.into());
                }
                if ui.get_printer_setting_busy() {
                    ui.set_printer_setting_busy(false);
                    ui.set_dialog_detail(tr(ui, "moonraker_disconnected").into());
                }
                ui.set_folder_picker_loading(false);
                ui.set_file_dialog_loading(false);
                ui.set_online(false);
                ui.set_ready(false);
                self.update_notifications(ui);
            }
            Event::Status(status) => self.set_status(ui, status),
            Event::Reply(tag, result) => self.reply(ui, &tag, result),
            Event::Notification(method, value) => match method.as_str() {
                "notify_filelist_changed" if self.page == "files" => {
                    self.requested_files.clear();
                    self.file_details.clear();
                    self.file_previews.clear();
                    self.refresh(ui);
                }
                "notify_job_queue_changed" => {
                    self.queue = value;
                    if self.page == "home" || self.page == "tune" && self.tab == "queue" {
                        self.call(ui, "server.job_queue.status", json!({}), "queue");
                    }
                }
                "notify_update_response" => {
                    if let Some(message) = value["message"].as_str() {
                        ui.set_message(message.into());
                    }
                    if value["complete"].as_bool() == Some(true) {
                        self.request_update_notifications();
                    }
                }
                "notify_update_refreshed" => self.set_updates(ui, value),
                "notify_gcode_response"
                    if value.as_str().is_some_and(|text| text.starts_with("!!")) =>
                {
                    ui.set_message(value.as_str().unwrap_or_default().into());
                }
                _ => {}
            },
        }
    }

    fn set_status(&mut self, ui: &Moondash, status: Map<String, Value>) {
        self.status = status;
        if ui.get_dialog_mode() == "temperature" {
            self.update_temperature_dialog(ui, &self.dialog.argument);
        }
        if ui.get_dialog_mode() == "speed"
            && self.edited_speed_percent.is_none()
            && let Some(percent) = self.speed_percent()
        {
            ui.set_speed_target(percent);
        }
        let webhooks = self.object("webhooks");
        let klippy = webhooks["state"].as_str().unwrap_or("connecting");
        ui.set_klipper_state(klippy.into());
        ui.set_ready(self.online && klippy == "ready");
        let stats = self.object("print_stats");
        let state = stats["state"].as_str().unwrap_or("standby");
        let filename = stats["filename"].as_str().unwrap_or_default().to_owned();
        let fraction = number(&self.object("display_status")["progress"])
            .filter(|progress| *progress > 0.)
            .or_else(|| number(&self.object("virtual_sdcard")["progress"]))
            .unwrap_or(0.)
            .clamp(0., 1.);
        let show_name = filename.rsplit('/').next().unwrap_or_default();
        ui.set_print_name(if show_name.to_ascii_lowercase().ends_with(".gcode") {
            show_name[..show_name.len() - ".gcode".len()].into()
        } else {
            show_name.into()
        });
        ui.set_display_message(
            self.object("display_status")["message"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .into(),
        );
        ui.set_printing(matches!(state, "printing" | "resuming" | "pausing"));
        ui.set_paused(state == "paused");
        ui.set_finished(matches!(state, "complete" | "cancelled"));
        ui.set_can_skip(!array(&self.object("exclude_object")["objects"]).is_empty());
        ui.set_has_afc(self.status.contains_key("AFC"));
        let homed = self.object("toolhead")["homed_axes"]
            .as_str()
            .unwrap_or_default();
        ui.set_x_homed(homed.contains('x'));
        ui.set_y_homed(homed.contains('y'));
        ui.set_z_homed(homed.contains('z'));
        ui.set_has_tilt(self.status.contains_key("z_tilt"));
        ui.set_has_qgl(
            self.status.contains_key("quad_gantry_level")
                || self.status.contains_key("gcode_macro QGL")
                || self.status.contains_key("gcode_macro QUAD_GANTRY_LEVEL"),
        );
        let gcode = self.object("gcode_move");
        let tool = self.object("toolhead")["extruder"]
            .as_str()
            .unwrap_or("extruder");
        ui.set_flow(
            format!(
                "{:.0}",
                number(&gcode["extrude_factor"]).unwrap_or(1.) * 100.
            )
            .into(),
        );
        ui.set_advance(
            format!(
                "{:.3}",
                number(&self.object(tool)["pressure_advance"]).unwrap_or(0.)
            )
            .into(),
        );
        ui.set_smooth(
            format!(
                "{:.3}",
                number(&self.object(tool)["smooth_time"])
                    .or_else(|| number(&self.object(tool)["pressure_advance_smooth_time"]))
                    .unwrap_or(0.04)
            )
            .into(),
        );
        ui.set_progress(fraction as f32);
        ui.set_layers(
            match (
                stats["info"]["current_layer"].as_u64(),
                stats["info"]["total_layer"].as_u64(),
            ) {
                (Some(current), Some(total)) => format!("{current}/{total}").into(),
                _ => "—".into(),
            },
        );
        let elapsed = number(&stats["print_duration"]).unwrap_or_default();
        ui.set_remaining(if fraction > 0. && elapsed > 0. {
            duration(elapsed / fraction - elapsed).into()
        } else {
            "—".into()
        });

        if self.filename != filename {
            self.filename = filename;
            ui.set_preview(Image::default());
            if !self.filename.is_empty() {
                self.call(
                    ui,
                    "server.files.metadata",
                    json!({"filename": self.filename}),
                    format!("preview:{}", self.filename),
                );
            }
        }
        self.update_shortcuts(ui);
        self.update_notifications(ui);
        if self.page == "tune" && self.tab == "afc" {
            self.update_afc(ui);
        }
        if self.page == "debug" {
            ui.set_diagnostics(
                serde_json::to_string_pretty(&self.status)
                    .unwrap_or_default()
                    .into(),
            );
        }
        if (self.page == "tune" && self.tab == "objects")
            || (self.page == "settings" && self.tab == "tools")
        {
            self.render_rows(ui);
        }
    }

    fn set_updates(&mut self, ui: &Moondash, updates: Value) {
        self.updates = updates;
        self.update_notifications(ui);
        if self.page == "settings" && self.tab == "updates" {
            self.render_rows(ui);
        }
    }

    fn update_notifications(&self, ui: &Moondash) {
        let tone = ui.global::<Palette>().get_accent();
        let brightness = 299 * u32::from(tone.red())
            + 587 * u32::from(tone.green())
            + 114 * u32::from(tone.blue());
        let action_ink = if brightness > 145_000 {
            rgb(0x000000)
        } else {
            rgb(0xffffff)
        };
        let mut rows = Vec::new();
        let mut push = |id: String, entry, icon| {
            if !self.dismissed_notifications.contains(&id) {
                rows.push(Notification {
                    id: id.into(),
                    action_ink,
                    entry: Row {
                        icon: icons::glyph(icon),
                        tone,
                        ..entry
                    },
                });
            }
        };

        if !self.online {
            let reason = ui.get_connection();
            push(
                "moonraker-connection".into(),
                row(
                    "Moonraker",
                    if reason.is_empty() {
                        tr(ui, "connecting_to_moonraker")
                    } else {
                        reason.as_str()
                    },
                    "reconnect-moonraker",
                    "",
                    tr(ui, "reconnect"),
                ),
                "information",
            );
        } else {
            for message in warning_messages(&self.moonraker_warnings) {
                push(
                    format!("moonraker-warning/{message}"),
                    info("Moonraker", message),
                    "alert-circle",
                );
            }

            let webhooks = self.object("webhooks");
            let state = webhooks["state"].as_str().unwrap_or("startup");
            let message = klipper_message(&webhooks["state_message"])
                .or_else(|| klipper_message(&self.object("print_stats")["message"]));
            if state != "ready" || message.is_some() {
                let (title, fallback, icon) = match state {
                    "error" => (
                        tr(ui, "klipper_error"),
                        tr(ui, "klipper_reported_an_error"),
                        "close-circle",
                    ),
                    "shutdown" => (
                        tr(ui, "klipper_shutdown"),
                        tr(ui, "klipper_has_shut_down"),
                        "close-circle",
                    ),
                    "disconnected" => ("Klipper", tr(ui, "klipper_is_disconnected"), "information"),
                    "ready" => ("Klipper", "", "close-circle"),
                    _ => (
                        tr(ui, "klipper_starting"),
                        tr(ui, "klipper_is_starting_up"),
                        "information",
                    ),
                };
                let mut entry = info(title, message.unwrap_or(fallback));
                if !ui.get_printing() && !ui.get_paused() {
                    entry.action = "restart-klipper".into();
                    entry.button = tr(ui, "restart").into();
                    entry = alternate(entry, "restart-firmware", tr(ui, "firmware_restart"));
                }
                push(
                    format!("klipper-status/{state}/{}", message.unwrap_or_default()),
                    entry,
                    icon,
                );
            }
            for warning_message in warning_messages(&self.object("configfile")["warnings"]) {
                if Some(warning_message) != message {
                    push(
                        format!("klipper-warning/{warning_message}"),
                        info(tr(ui, "klipper_warning"), warning_message),
                        "alert-circle",
                    );
                }
            }

            if let Some(message) = afc_notification(self.object("AFC")) {
                push(
                    format!("afc/{message}"),
                    row("AFC", message, "afc-reset", "", tr(ui, "clear")),
                    "alert-circle",
                );
            }
            let count = self.updates["version_info"]
                .as_object()
                .into_iter()
                .flat_map(|components| components.values())
                .filter(|component| update_available(component))
                .count();
            if count > 0 {
                push(
                    format!("updates/{count}"),
                    row(
                        tr(ui, "updates"),
                        if count == 1 {
                            tr(ui, "1_update_available").to_owned()
                        } else {
                            tr(ui, "count_updates_available").replace("{count}", &count.to_string())
                        },
                        "show-updates",
                        "",
                        tr(ui, "settings"),
                    ),
                    "update",
                );
            }
        }

        set_model(&ui.get_notifications(), &rows, |model| {
            ui.set_notifications(model)
        });
    }

    fn dismiss_notification(&mut self, ui: &Moondash, id: &str) {
        if !self.dismissed_notifications.insert(id.to_owned()) {
            return;
        }
        self.update_notifications(ui);
        if self.online && is_persistent_warning(id) {
            self.call(
                ui,
                "server.database.post_item",
                json!({"namespace": "moondash", "key": [NOTIFICATION_DISMISSALS, id], "value": true}),
                "notification-dismissal-save",
            );
        }
    }

    fn object(&self, key: &str) -> &Value {
        self.status.get(key).unwrap_or(&Value::Null)
    }

    fn heater_rows<'a>(&'a self, ui: &'a Moondash) -> impl Iterator<Item = Row> + 'a {
        self.status
            .iter()
            .filter(|(name, _)| {
                *name == "heater_bed"
                    || name.starts_with("extruder") && !name.contains('_')
                    || name.starts_with("heater_generic ")
            })
            .filter_map(move |(name, data)| {
                let current = number(&data["temperature"])?;
                let target = number(&data["target"]).unwrap_or_default();
                Some(row(
                    format!("{name}  {current:.0}°C"),
                    tr(ui, "heater_target").replace("{temperature}", &format!("{target:.0}")),
                    "heater",
                    name,
                    tr(ui, "set"),
                ))
            })
    }

    fn update_shortcuts(&self, ui: &Moondash) {
        let ink = if ui.get_dark() {
            slint::Color::from_rgb_u8(255, 255, 255)
        } else {
            slint::Color::from_rgb_u8(25, 25, 25)
        };
        let heater = |name: &str, caption: &str, icon: &str| {
            let current = number(&self.object(name)["temperature"]);
            let target = number(&self.object(name)["target"]).unwrap_or_default();
            let mut entry = row(
                caption,
                current
                    .map(|temp| format!("{temp:.0}°C"))
                    .unwrap_or_else(|| "--°C".into()),
                "heater",
                name,
                tr(ui, "set"),
            );
            entry.icon = icons::glyph(icon);
            entry.tone = match current {
                Some(temp) if target > 0. && temp < target - 5. => rgb(0xcf6679),
                Some(temp) if target > 0. && temp > target + 5. => rgb(0x2196f3),
                _ => ink,
            };
            entry
        };
        let tool = self.object("toolhead")["extruder"]
            .as_str()
            .unwrap_or("extruder");
        let bed = tr(ui, "heater_bed");
        let mut controls = vec![
            heater(tool, tr(ui, "extruder"), "printer-3d-nozzle-heat"),
            heater("heater_bed", bed, "radiator"),
        ];
        let mut shortcuts = controls.clone();
        for control in &mut shortcuts {
            control.title = control.detail.clone();
        }
        for key in self.status.keys().filter(|name| {
            name.starts_with("heater_generic ")
                || name.starts_with("extruder") && !name.contains('_') && *name != tool
        }) {
            let name = key
                .split_once(' ')
                .map_or(key.as_str(), |(_, suffix)| suffix);
            controls.push(heater(key, name, "thermometer"));
        }
        let first_fan = controls.len();
        for (key, data) in &self.status {
            let (name, action) = if key == "fan" {
                (tr(ui, "part_fan"), "fan")
            } else if let Some((kind, name)) = key.split_once(' ') {
                match kind {
                    "fan_generic" => (name, "fan"),
                    "temperature_fan" => (name, "temperature-fan"),
                    "heater_fan" | "controller_fan" => (name, ""),
                    _ => continue,
                }
            } else {
                continue;
            };
            let speed = number(&data["speed"]);
            let rpm = number(&data["rpm"]);
            let Some(detail) = speed
                .map(|speed| format!("{:.0}%", speed * 100.))
                .or_else(|| rpm.map(|rpm| if rpm > 0. { "100%" } else { "0%" }.to_owned()))
            else {
                continue;
            };
            let mut entry = row(name, detail, action, key, tr(ui, "set"));
            entry.active = speed.is_some_and(|speed| speed > 0.) || rpm.is_some_and(|rpm| rpm > 0.);
            entry.icon = icons::glyph("fan");
            entry.tone = ink;
            controls.push(entry);
        }
        controls[first_fan..].sort_by_key(|fan| fan.action.is_empty());
        let factor = number(&self.object("gcode_move")["speed_factor"]);
        let mut speed = row(
            factor
                .map(|speed| format!("{:.0}%", speed * 100.))
                .unwrap_or_else(|| "--".into()),
            "",
            "speed",
            "",
            tr(ui, "set"),
        );
        speed.icon = icons::glyph(match factor {
            Some(speed) if speed < 1. => "speedometer-slow",
            Some(speed) if speed > 1. => "speedometer",
            _ => "speedometer-medium",
        });
        speed.tone = ink;
        shortcuts.push(speed);
        if self.status.contains_key("AFC") {
            shortcuts.push(Row {
                icon: icons::glyph("printer-3d-nozzle"),
                tone: ink,
                ..row("AFC", "", "show-afc", "", "")
            });
        }

        let macros: Vec<_> = array(&self.config["shortcutbuttons"])
            .iter()
            .take(3)
            .filter_map(|item| {
                let name = item["name"].as_str()?;
                let mut button = row(name, "", "shortcut", name, tr(ui, "run"));
                button.active = shortcut_active(&self.status, item);
                button.icon = icons::glyph(item["icon"].as_str().unwrap_or_default());
                if button.icon.is_empty() {
                    button.icon = icons::glyph("flash");
                }
                Some(button)
            })
            .collect();

        let mut sensors: Vec<_> = self
            .status
            .iter()
            .filter_map(|(key, data)| {
                let name = filament_sensor_name(key)?;
                Some(FilamentSensor {
                    key: key.as_str().into(),
                    name: name.replace('_', " ").into(),
                    enabled: filament_sensor_enabled(data),
                    detected: data["filament_detected"]
                        .as_bool()
                        .or_else(|| data["detected"].as_bool())
                        .or_else(|| data["state"].as_bool())
                        .unwrap_or_default(),
                })
            })
            .collect();
        sensors.sort_by_cached_key(|sensor| sensor.name.to_lowercase());
        let mut details = Vec::new();
        if self.config["styling"]["advanced_states"].as_bool() == Some(true) {
            let position = &self.object("gcode_move")["gcode_position"];
            for (axis, index) in [("X", 0), ("Y", 1), ("Z", 2)] {
                if let Some(value) = number(&position[index]) {
                    details.push(info(axis, format!("{value:.2} mm")));
                }
            }
        }

        set_model(&ui.get_shortcuts(), &shortcuts, |model| {
            ui.set_shortcuts(model)
        });
        set_model(&ui.get_motion_controls(), &controls, |model| {
            ui.set_motion_controls(model)
        });
        set_model(&ui.get_sensors(), &sensors, |model| ui.set_sensors(model));
        set_model(&ui.get_macros(), &macros, |model| ui.set_macros(model));
        set_model(&ui.get_details(), &details, |model| ui.set_details(model));
    }

    fn update_afc(&mut self, ui: &Moondash) {
        let root = self.object("AFC");
        let mut ordered_units: Vec<String> = array(&root["units"])
            .iter()
            .filter_map(|unit| {
                let id = unit.as_str()?;
                Some(id.split_once(' ').map_or(id, |(_, unit)| unit).to_owned())
            })
            .collect();
        let mut unit_lanes: HashMap<String, Vec<(String, &Value)>> = HashMap::new();
        for (key, lane) in &self.status {
            let Some(name) = key.strip_prefix("AFC_stepper ") else {
                continue;
            };
            let unit = lane["unit"].as_str().unwrap_or("AFC");
            if !ordered_units.iter().any(|candidate| candidate == unit) {
                ordered_units.push(unit.into());
            }
            unit_lanes
                .entry(unit.to_owned())
                .or_default()
                .push((name.to_owned(), lane));
        }
        let chosen = ordered_units
            .iter()
            .find(|unit| **unit == self.afc_unit)
            .or_else(|| ordered_units.first())
            .cloned()
            .unwrap_or_default();
        let lanes: Vec<Lane> = unit_lanes
            .get(&chosen)
            .into_iter()
            .flatten()
            .map(|(name, lane)| {
                let loaded = lane["tool_loaded"].as_bool().unwrap_or_default()
                    || root["current_load"].as_str() == Some(name)
                    || root["current_lane"].as_str() == Some(name);
                let has_filament = lane["prep"].as_bool().unwrap_or_default()
                    || lane["load"].as_bool().unwrap_or_default()
                    || lane["loaded_to_hub"].as_bool().unwrap_or_default()
                    || loaded;
                let color = if has_filament {
                    config_color(&lane["color"], 0x383838)
                } else {
                    rgb(0x383838)
                };
                let brightness = 299 * u32::from(color.red())
                    + 587 * u32::from(color.green())
                    + 114 * u32::from(color.blue());
                let material = lane["material"]
                    .as_str()
                    .filter(|material| !material.is_empty());
                Lane {
                    name: name.as_str().into(),
                    material: if has_filament {
                        material.unwrap_or(name)
                    } else {
                        tr(ui, "empty")
                    }
                    .into(),
                    weight: number(&lane["weight"])
                        .filter(|_| has_filament)
                        .map(|weight| format!("{weight:.0} g").into())
                        .unwrap_or_default(),
                    color,
                    ink: if brightness >= 138_000 {
                        rgb(0x161616)
                    } else {
                        rgb(0xffffff)
                    },
                    loaded,
                    has_filament,
                    has_runout: lane["runout_lane"].as_str().is_some_and(|runout| {
                        !runout.is_empty() && !runout.eq_ignore_ascii_case("none")
                    }),
                }
            })
            .collect();
        let units: Vec<Row> = ordered_units
            .iter()
            .map(|unit| Row {
                active: unit == &chosen,
                ..info(unit.as_str(), "")
            })
            .collect();
        self.afc_unit = chosen;
        set_model(&ui.get_afc_units(), &units, |model| ui.set_afc_units(model));
        set_model(&ui.get_afc_lanes(), &lanes, |model| ui.set_afc_lanes(model));
    }

    fn update_jobs(&self, ui: &Moondash) {
        let jobs: Vec<Row> = array(&self.queue["queued_jobs"])
            .iter()
            .filter_map(|job| {
                let name = job["filename"].as_str()?;
                Some(row(name, "", "show-queue", "", ""))
            })
            .collect();
        set_model(&ui.get_jobs(), &jobs, |model| ui.set_jobs(model));
    }

    fn load_visible_file_details(&mut self, ui: &Moondash) {
        use slint::Model;
        let capacity = (ui.get_file_rows() * ui.get_file_columns()).max(1) as usize;
        let rows = ui.get_rows();
        let last_page = rows.row_count().saturating_sub(1) / capacity;
        if ui.get_file_page() as usize > last_page {
            ui.set_file_page(last_page as i32);
        }
        let start = ui.get_file_page() as usize * capacity;
        for index in start..(start + capacity).min(rows.row_count()) {
            let Some(entry) = rows.row_data(index) else {
                continue;
            };
            if entry.action != "file" || self.requested_files.contains(entry.argument.as_str()) {
                continue;
            }
            let path = entry.argument.to_string();
            if self
                .calls
                .try_send(Call::new(
                    "server.files.metadata",
                    json!({"filename": path}),
                    format!("card:{path}"),
                ))
                .is_err()
            {
                break;
            }
            self.requested_files.insert(path);
        }
    }
}

fn tr<'a>(ui: &Moondash, key: &'a str) -> &'a str {
    locale::text(key, ui.global::<I18n>().get_german())
}

fn row(
    title: impl Into<slint::SharedString>,
    detail: impl Into<slint::SharedString>,
    action: &str,
    argument: &str,
    button: &str,
) -> Row {
    Row {
        title: title.into(),
        detail: detail.into(),
        action: action.into(),
        argument: argument.into(),
        button: button.into(),
        ..Default::default()
    }
}

fn info(title: impl Into<slint::SharedString>, detail: impl Into<slint::SharedString>) -> Row {
    row(title, detail, "", "", "")
}

fn alternate(mut row: Row, action: &str, button: &str) -> Row {
    row.alternate = action.into();
    row.alternate_button = button.into();
    row
}

fn set_model<T: Clone + PartialEq + 'static>(
    previous: &ModelRc<T>,
    next: &[T],
    replace: impl FnOnce(ModelRc<T>),
) {
    use slint::Model;
    if let Some(model) = previous.as_any().downcast_ref::<VecModel<T>>()
        && model.row_count() == next.len()
    {
        for (index, row) in next.iter().enumerate() {
            if model.row_data(index).as_ref() != Some(row) {
                model.set_row_data(index, row.clone());
            }
        }
    } else {
        replace(ModelRc::new(VecModel::from(next.to_vec())));
    }
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|value| value.is_finite())
}

fn filament_sensor_name(key: &str) -> Option<&str> {
    let name = key
        .strip_prefix("filament_switch_sensor ")
        .or_else(|| key.strip_prefix("filament_motion_sensor "))?;
    (!name.is_empty() && !name.to_ascii_lowercase().starts_with("afc")).then_some(name)
}

fn filament_sensor_enabled(data: &Value) -> bool {
    data["enabled"]
        .as_bool()
        .or_else(|| data["sensor_enabled"].as_bool())
        .unwrap_or(true)
}

fn warning_messages(warnings: &Value) -> impl Iterator<Item = &str> {
    let mut seen = HashSet::new();
    array(warnings)
        .iter()
        .filter_map(|warning| warning.as_str().or_else(|| warning["message"].as_str()))
        .map(str::trim)
        .filter(move |message| !message.is_empty() && seen.insert(*message))
}

fn is_persistent_warning(id: &str) -> bool {
    id.starts_with("klipper-warning/") || id.starts_with("moonraker-warning/")
}

fn klipper_message(value: &Value) -> Option<&str> {
    value.as_str().map(str::trim).filter(|message| {
        !message.is_empty()
            && !message.eq_ignore_ascii_case("ready")
            && !message.eq_ignore_ascii_case("printer is ready")
    })
}

fn afc_notification(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    [
        "message",
        "error",
        "alert",
        "current_message",
        "last_message",
        "state_message",
        "stateMessage",
    ]
    .into_iter()
    .filter_map(|key| object.get(key)?.as_str().map(str::trim))
    .find(|message| !message.is_empty())
    .or_else(|| object.values().find_map(afc_notification))
}

fn update_available(component: &Value) -> bool {
    let version = |field| component[field].as_str().filter(|value| !value.is_empty());
    let dirty = component["is_dirty"].as_bool() == Some(true)
        || component["dirty"].as_bool() == Some(true)
        || ["version", "remote_version", "full_version_string"]
            .into_iter()
            .filter_map(version)
            .any(|value| value.to_ascii_lowercase().contains("dirty"));
    if dirty {
        return false;
    }
    if component["configured_type"].as_str() == Some("system") {
        return component["package_count"].as_u64().unwrap_or_default() > 0;
    }
    component["commits_behind_count"]
        .as_u64()
        .unwrap_or_default()
        > 0
        || version("current_hash")
            .zip(version("remote_hash"))
            .is_some_and(|(current, remote)| current != remote)
        || version("version")
            .zip(version("remote_version"))
            .is_some_and(|(current, remote)| current != remote)
}

fn format_speed_percent(percent: i32) -> String {
    if percent < 1_000 {
        return format!("{percent}%");
    }

    let thousands = percent / 1_000;
    let hundredths = percent % 1_000 / 10;
    match hundredths {
        0 => format!("{thousands}k"),
        n if n % 10 == 0 => format!("{thousands}.{}k", n / 10),
        n => format!("{thousands}.{n:02}k"),
    }
}

fn gcode_path(path: &str) -> String {
    if path.is_empty() {
        "gcodes".to_owned()
    } else {
        format!("gcodes/{path}")
    }
}

fn join_path(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

fn duration(seconds: f64) -> String {
    let minutes = (seconds.max(0.) / 60.).ceil() as u64;
    if minutes >= 60 {
        format!("{}h {:02}m", minutes / 60, minutes % 60)
    } else {
        format!("{minutes}m")
    }
}

fn rgb(color: u32) -> slint::Color {
    slint::Color::from_rgb_u8((color >> 16) as u8, (color >> 8) as u8, color as u8)
}

fn config_color(value: &Value, default: u32) -> slint::Color {
    rgb(value.as_str().and_then(theme::parse_rgb).unwrap_or(default))
}

fn config_int(value: &Value, max: i32) -> Option<i32> {
    let value = value.as_i64().or_else(|| value.as_str()?.parse().ok())?;
    (1..=i64::from(max))
        .contains(&value)
        .then_some(value as i32)
}

fn web_qr(url: &str) -> Image {
    use qrcode::{Color, QrCode};
    let Ok(code) = QrCode::new(url) else {
        return Image::default();
    };
    let dimension = code.width();
    let size = ((dimension + 8) * 4) as u32;
    let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(size, size);
    for (index, pixel) in pixels.make_mut_slice().iter_mut().enumerate() {
        let col = (index % size as usize) / 4;
        let row = (index / size as usize) / 4;
        let inside = (4..dimension + 4).contains(&col) && (4..dimension + 4).contains(&row);
        let gray = if inside && code[(col - 4, row - 4)] == Color::Dark {
            0
        } else {
            255
        };
        *pixel = slint::Rgb8Pixel {
            r: gray,
            g: gray,
            b: gray,
        };
    }
    Image::from_rgb8(pixels)
}

fn valid_leaf(name: &str) -> bool {
    print_files::valid_path(name)
        && !name.contains('/')
        && !name.starts_with('.')
        && name == name.trim()
}

fn without_last(value: slint::SharedString) -> String {
    let mut value = value.to_string();
    value.pop();
    value
}

fn folder_label<'a>(ui: &Moondash, path: &'a str) -> &'a str {
    if path.is_empty() {
        tr(ui, "root_folder")
    } else {
        path
    }
}

fn folder_entries(listing: &Value) -> Vec<String> {
    let mut entries: Vec<_> = array(&listing["dirs"])
        .iter()
        .filter_map(|item| {
            item.as_str()
                .or_else(|| item["dirname"].as_str())
                .or_else(|| item["name"].as_str())
        })
        .filter(|name| valid_leaf(name))
        .map(str::to_owned)
        .collect();
    entries.sort_by_cached_key(|name| name.to_lowercase());
    entries.dedup();
    entries
}

fn display_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.)
    }
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl State {
    fn heater_limit(&self, heater: &str) -> f64 {
        let settings = &self.object("configfile")["settings"][heater];
        number(&settings["max_temp"])
            .filter(|limit| *limit > 0.)
            .unwrap_or_else(|| match heater {
                "heater_bed" => 120.,
                name if name.starts_with("temperature_fan ") => 120.,
                name if name.starts_with("extruder") => 300.,
                _ => 150.,
            })
    }

    fn update_temperature_dialog(&self, ui: &Moondash, heater: &str) {
        let state = self.object(heater);
        let maximum = self.heater_limit(heater).floor() as i32;
        let target = number(&state["target"]).unwrap_or_default().round() as i32;
        ui.set_temperature_max(maximum);
        ui.set_temperature_target(target);
        ui.set_dialog_current(
            number(&state["temperature"])
                .map(|temperature| format!("{temperature:.0}°C"))
                .unwrap_or_else(|| "--°C".into())
                .into(),
        );
    }

    fn set_temperature(&self, ui: &Moondash, heater: &str, input: &str) {
        let Ok(requested) = input.parse::<i32>() else {
            return;
        };
        let target = requested.clamp(0, self.heater_limit(heater).floor() as i32);
        let (kind, name) = heater.split_once(' ').unwrap_or(("", heater));
        let name = gcode_arg(name);
        let script = if kind == "temperature_fan" {
            format!("SET_TEMPERATURE_FAN_TARGET TEMPERATURE_FAN={name} TARGET={target}")
        } else {
            format!("SET_HEATER_TEMPERATURE HEATER={name} TARGET={target}")
        };
        self.submit_printer_setting(ui, script);
    }

    fn set_fan(&self, ui: &Moondash, fan: &str, input: &str) {
        let Ok(target) = input.parse::<u32>() else {
            return;
        };
        let target = target.min(100);
        let script = if fan == "fan" {
            format!("M106 S{}", (target * 255 + 50) / 100)
        } else if let Some(name) = fan.strip_prefix("fan_generic ") {
            format!(
                "SET_FAN_SPEED FAN={} SPEED={:.2}",
                gcode_arg(name),
                f64::from(target) / 100.
            )
        } else {
            return;
        };
        self.submit_printer_setting(ui, script);
    }

    fn speed_percent(&self) -> Option<i32> {
        number(&self.object("gcode_move")["speed_factor"])
            .map(|factor| ((factor * 100.).round() as i32).max(1))
    }

    fn speed_acceleration(&self) -> Option<f64> {
        number(&self.object("toolhead")["max_accel"]).filter(|value| *value > 0.)
    }

    fn save_speed_settings(&self, ui: &Moondash) {
        let input = ui.get_speed_acceleration();
        let input = input.trim();
        let acceleration = input
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.);
        if acceleration.is_none()
            && (!input.is_empty() || self.initial_speed_acceleration.is_some())
        {
            ui.set_dialog_detail(tr(ui, "enter_an_acceleration_greater_than_0").into());
            return;
        }

        let mut commands = Vec::with_capacity(2);
        if let Some(acceleration) = acceleration
            && Some(acceleration) != self.initial_speed_acceleration
            && Some(acceleration) != self.speed_acceleration()
        {
            commands.push(format!("SET_VELOCITY_LIMIT ACCEL={acceleration}"));
        }
        if let Some(percent) = self.edited_speed_percent
            && Some(percent) != self.speed_percent()
        {
            commands.push(format!("M220 S{percent}"));
        }
        if commands.is_empty() {
            ui.set_dialog_mode("".into());
        } else {
            self.submit_printer_setting(ui, commands.join("\n"));
        }
    }

    fn submit_printer_setting(&self, ui: &Moondash, script: String) {
        if ui.get_printer_setting_busy() || !ui.get_ready() {
            return;
        }
        ui.set_dialog_detail("".into());
        if self.try_call(
            ui,
            "printer.gcode.script",
            json!({"script": script}),
            "printer-setting",
        ) {
            ui.set_printer_setting_busy(true);
        }
    }

    fn render_rows(&self, ui: &Moondash) {
        let mut rows = Vec::new();
        match (self.page.as_str(), self.tab.as_str()) {
            ("files", _) => {
                let mut path = String::new();
                let crumbs: Vec<_> = self
                    .folder
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .map(|part| {
                        path = join_path(&path, part);
                        row(part, "", "folder", &path, "")
                    })
                    .collect();
                set_model(&ui.get_breadcrumbs(), &crumbs, |model| {
                    ui.set_breadcrumbs(model)
                });
                rows = print_files::entries(
                    &self.file_listing,
                    &self.folder,
                    &self.filter,
                    self.file_sort,
                );
                for row in &mut rows {
                    if let Some(detail) = self.file_details.get(row.argument.as_str()) {
                        row.detail = print_files::estimated_time(detail)
                            .unwrap_or_default()
                            .into();
                    }
                    if row.action == "file" {
                        print_files::history(row, array(&self.file_history["jobs"]));
                        row.active = self.requested_files.contains(row.argument.as_str())
                            && !self.file_details.contains_key(row.argument.as_str());
                    }
                    if let Some(preview) = self.file_previews.get(row.argument.as_str()) {
                        row.preview = preview.clone();
                    }
                }
            }
            ("tune", "objects") => {
                let data = self.object("exclude_object");
                for object in array(&data["objects"]) {
                    let Some(name) = object["name"].as_str() else {
                        continue;
                    };
                    if array(&data["excluded_objects"])
                        .iter()
                        .any(|value| value.as_str() == Some(name))
                    {
                        rows.push(info(name, tr(ui, "excluded")));
                    } else {
                        rows.push(row(name, "", "exclude", name, tr(ui, "skip")));
                    }
                }
                if rows.is_empty() {
                    rows.push(info(tr(ui, "no_printable_objects"), ""));
                }
            }
            ("tune", "queue") => {
                let state = self.queue["queue_state"].as_str().unwrap_or("—");
                rows.push(if state == "paused" {
                    row(
                        tr(ui, "job_queue_warteschlange"),
                        state,
                        "queue-start",
                        "",
                        tr(ui, "start_queue"),
                    )
                } else {
                    row(
                        tr(ui, "job_queue_warteschlange"),
                        state,
                        "queue-pause",
                        "",
                        tr(ui, "pause_queue"),
                    )
                });
                for job in array(&self.queue["queued_jobs"]) {
                    if let (Some(id), Some(filename)) =
                        (job["job_id"].as_str(), job["filename"].as_str())
                    {
                        rows.push(alternate(
                            row(filename, "", "file", filename, tr(ui, "details")),
                            &format!("queue-remove:{id}"),
                            tr(ui, "remove"),
                        ));
                    }
                }
            }
            ("settings", "config") => {
                let system = &self.config["system"];
                let styling = &self.config["styling"];
                let mut dark = row(
                    tr(ui, "dark_mode"),
                    "",
                    "config-toggle",
                    "styling.darkmode",
                    tr(ui, "on"),
                );
                dark.active = styling["darkmode"].as_bool().unwrap_or(true);
                if !dark.active {
                    dark.button = tr(ui, "off").into();
                }
                rows.push(dark);
                rows.push(alternate(
                    row(
                        tr(ui, "colors"),
                        "",
                        "config-color",
                        "styling.primary",
                        tr(ui, "primary"),
                    ),
                    "config-secondary",
                    tr(ui, "secondary"),
                ));
                let mut advanced = row(
                    tr(ui, "advanced_state"),
                    "",
                    "config-toggle",
                    "styling.advanced_states",
                    tr(ui, "off"),
                );
                advanced.active = styling["advanced_states"].as_bool().unwrap_or(false);
                if advanced.active {
                    advanced.button = tr(ui, "on").into();
                }
                rows.push(advanced);
                rows.push(row(
                    tr(ui, "language"),
                    "",
                    "config-language",
                    system["language"].as_str().unwrap_or("auto"),
                    "",
                ));
                rows.push(row(
                    tr(ui, "display_profile"),
                    "",
                    "config-profile",
                    system["display_profile"]
                        .as_str()
                        .or_else(|| styling["display_profile"].as_str())
                        .unwrap_or("auto"),
                    "",
                ));
                let mut idle = row(
                    tr(ui, "display_idle_timeout"),
                    "",
                    "config-toggle",
                    "system.use_idle_timeout",
                    tr(ui, "on"),
                );
                idle.active = system["use_idle_timeout"].as_bool().unwrap_or(true);
                if !idle.active {
                    idle.button = tr(ui, "off").into();
                }
                rows.push(idle);
                if system["use_idle_timeout"].as_bool().unwrap_or(true) {
                    let timeout = system["idle_timeout"].as_u64().unwrap_or(900);
                    rows.push(row(
                        tr(ui, "idle_timeout"),
                        format!("{timeout} s"),
                        "config-timeout",
                        &timeout.to_string(),
                        "",
                    ));
                }
                rows.push(row(
                    tr(ui, "moonraker_ip"),
                    self.endpoint.borrow().as_str(),
                    "config-edit",
                    "websocket.ip",
                    tr(ui, "edit"),
                ));
            }
            ("settings", "shortcuts") => {
                for shortcut in array(&self.config["shortcutbuttons"]) {
                    let name = shortcut["name"].as_str().unwrap_or_default();
                    let mut entry = row(
                        name,
                        shortcut["macro_inactive"].as_str().unwrap_or_default(),
                        "edit-shortcut",
                        name,
                        tr(ui, "edit_shortcut"),
                    );
                    entry.icon = icons::glyph(shortcut["icon"].as_str().unwrap_or_default());
                    rows.push(entry);
                }
                if rows.len() < 3 {
                    rows.push(row(tr(ui, "add_shortcut"), "", "add-shortcut", "", "+"));
                }
            }
            ("settings", "network") => self.update_network_models(ui),
            ("settings", "tools") => match self.tool_view.as_str() {
                "mesh" => {
                    rows.push(row(
                        tr(ui, "bed_mesh"),
                        self.object("bed_mesh")["profile_name"]
                            .as_str()
                            .unwrap_or_default(),
                        "mesh-info",
                        "",
                        tr(ui, "inspect"),
                    ));
                    rows.push(alternate(
                        row(
                            tr(ui, "bed_mesh_calibration"),
                            "",
                            "mesh-calibrate",
                            "",
                            tr(ui, "calibrate"),
                        ),
                        "mesh-clear",
                        tr(ui, "clear"),
                    ));
                    if let Some(profiles) = self.object("bed_mesh")["profiles"].as_object() {
                        for name in profiles.keys() {
                            rows.push(alternate(
                                row(
                                    name.as_str(),
                                    tr(ui, "bed_mesh_profile"),
                                    "mesh-load",
                                    name,
                                    tr(ui, "load"),
                                ),
                                "mesh-remove",
                                tr(ui, "remove"),
                            ));
                        }
                    }
                    rows.push(row(
                        tr(ui, "save_printer_configuration"),
                        tr(ui, "klipper_restarts_after_saving"),
                        "save-printer",
                        "",
                        tr(ui, "save"),
                    ));
                }
                "pid" => {
                    for heater in self.heater_rows(ui) {
                        rows.push(row(
                            heater.argument.clone(),
                            tr(ui, "pid_calibration"),
                            "pid",
                            heater.argument.as_str(),
                            tr(ui, "tune"),
                        ));
                    }
                    rows.push(row(
                        tr(ui, "save_printer_configuration"),
                        tr(ui, "klipper_restarts_after_saving"),
                        "save-printer",
                        "",
                        tr(ui, "save"),
                    ));
                }
                "camera" => {}
                _ => {
                    for (title, action, icon) in [
                        (tr(ui, "bed_mesh"), "tool-mesh", "grid"),
                        (tr(ui, "pid_tuning"), "tool-pid", "tune-variant"),
                        (tr(ui, "webcam"), "tool-camera", "webcam"),
                    ] {
                        rows.push(Row {
                            icon: icons::glyph(icon),
                            ..row(title, "", action, "", "")
                        });
                    }
                }
            },
            ("settings", "host") => {
                rows.push(row(
                    "Moondash",
                    version::current_version(),
                    "refresh",
                    "",
                    tr(ui, "refresh"),
                ));
                let system = &self.host["info"]["system_info"];
                let stats = &self.host["stats"];
                rows.push(info(
                    tr(ui, "host"),
                    system["distribution"]["pretty_name"]
                        .as_str()
                        .unwrap_or("—"),
                ));
                rows.push(info(
                    "CPU",
                    system["cpu_info"]["cpu_desc"].as_str().unwrap_or("—"),
                ));
                if let Some(cpu) = number(&stats["system_cpu_usage"]["cpu"]) {
                    rows.push(info(tr(ui, "cpu_usage"), format!("{cpu:.1}%")));
                }
                if let Some(memory) = stats["system_memory"].as_object() {
                    rows.push(info(
                        tr(ui, "memory"),
                        format!(
                            "{} / {}",
                            display_size(memory["used"].as_u64().unwrap_or_default() * 1024),
                            display_size(memory["total"].as_u64().unwrap_or_default() * 1024)
                        ),
                    ));
                }
                rows.push(row(
                    tr(ui, "g_code_storage"),
                    "",
                    "storage",
                    "",
                    tr(ui, "inspect"),
                ));
                rows.push(row(
                    tr(ui, "print_history"),
                    "",
                    "history",
                    "",
                    tr(ui, "open"),
                ));
                let mcus: Vec<_> = self
                    .status
                    .iter()
                    .filter(|(name, _)| name.as_str() == "mcu" || name.starts_with("mcu "))
                    .map(|(name, data)| {
                        let version = data["mcu_version"].as_str().unwrap_or_default();
                        let chipset = data["mcu_constants"]["MCU"].as_str().unwrap_or_default();
                        let detail = if chipset.is_empty() {
                            version.into()
                        } else {
                            format!("{chipset} · {version}")
                        };
                        Row {
                            icon: icons::glyph("chip"),
                            ..info(name.as_str(), detail)
                        }
                    })
                    .collect();
                set_model(&ui.get_trailing_rows(), &mcus, |model| {
                    ui.set_trailing_rows(model)
                });
            }
            ("settings", "updates") => {
                rows.push(alternate(
                    row(
                        tr(ui, "software_updates"),
                        "",
                        "update-refresh",
                        "",
                        tr(ui, "check"),
                    ),
                    "update-all",
                    tr(ui, "update_all"),
                ));
                if let Some(components) = self.updates["version_info"].as_object() {
                    for (name, data) in components {
                        let installed = data["version"].as_str().unwrap_or("—");
                        let available = data["remote_version"].as_str().unwrap_or(installed);
                        let detail = if installed == available {
                            installed.to_owned()
                        } else {
                            format!("{installed} → {available}")
                        };
                        let updatable = installed != available
                            || data["package_count"].as_u64().unwrap_or_default() > 0;
                        rows.push(if updatable {
                            row(name.as_str(), detail, "update", name, tr(ui, "update"))
                        } else {
                            info(name.as_str(), detail)
                        });
                    }
                }
                rows.push(row(
                    tr(ui, "moondash_service"),
                    tr(ui, "after_a_moondash_package_update"),
                    "restart-service",
                    "",
                    tr(ui, "restart"),
                ));
            }
            ("debug", _) => ui.set_diagnostics(
                serde_json::to_string_pretty(&self.status)
                    .unwrap_or_default()
                    .into(),
            ),
            _ => {}
        }
        for entry in &mut rows {
            if !entry.icon.is_empty() {
                continue;
            }
            entry.icon = icons::glyph(match entry.action.as_str() {
                "webcam" => "webcam",
                "mesh-info" | "mesh-calibrate" => "grid",
                "pid" => "tune-variant",
                "history" => "clock-outline",
                "restart-klipper" | "restart-service" => "restart",
                "reboot" | "sleep" => "power",
                "config-edit" => "pencil",
                "add-shortcut" => "lightning-bolt",
                _ => "",
            });
        }
        set_model(&ui.get_rows(), &rows, |model| ui.set_rows(model));
    }

    fn show_file_details(&mut self, ui: &Moondash, path: &str) {
        self.metadata = self.file_details.get(path).cloned().unwrap_or_default();
        self.update_print_details(ui);
        if let Some(image) = self.file_previews.get(path) {
            ui.set_dialog_image(image.clone());
        }
        ui.set_file_dialog_loading(ui.get_dialog_image().size().width == 0);
        if !self.try_call(
            ui,
            "server.files.metadata",
            json!({"filename":path}),
            format!("file:{path}"),
        ) {
            ui.set_file_dialog_loading(false);
        }
    }

    fn update_print_details(&self, ui: &Moondash) {
        ui.set_dialog_time(
            print_files::estimated_time(&self.metadata)
                .unwrap_or_default()
                .into(),
        );
        ui.set_dialog_weight(
            print_files::filament_weight(&self.metadata)
                .unwrap_or_default()
                .into(),
        );
        let tools: Vec<_> = if self.status.contains_key("AFC") {
            array(&self.metadata["filament_weights"])
                .iter()
                .enumerate()
                .filter_map(|(index, weight)| {
                    let used = number(weight)
                        .or_else(|| weight.as_str()?.parse().ok())
                        .is_some_and(|weight| weight > 0.);
                    if !used {
                        return None;
                    }
                    let name = format!("T{index}");
                    let selected = self.status.iter().find_map(|(key, lane)| {
                        let key = key.strip_prefix("AFC_stepper ")?;
                        lane["map"]
                            .as_str()
                            .is_some_and(|mapped| mapped.eq_ignore_ascii_case(&name))
                            .then_some((key, lane))
                    });
                    let lane_color = selected.map_or(rgb(0x434343), |(_, lane)| {
                        config_color(&lane["color"], 0x434343)
                    });
                    let luminance = u32::from(lane_color.red()) * 213
                        + u32::from(lane_color.green()) * 715
                        + u32::from(lane_color.blue()) * 72;
                    Some(PrintTool {
                        name: name.into(),
                        color: config_color(&self.metadata["filament_colors"][index], 0x434343),
                        lane: selected.map_or("—", |(key, _)| key).into(),
                        lane_color,
                        lane_ink: if luminance > 165_000 {
                            rgb(0)
                        } else {
                            rgb(0xffffff)
                        },
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        set_model(&ui.get_print_tools(), &tools, |model| {
            ui.set_print_tools(model)
        });

        let timelapse_keys = [
            "timelapse_settings",
            "gcode_macro TIMELAPSE_TAKE_FRAME",
            "gcode_macro timelapse_take_frame",
            "timelapse",
            "timelapse_mode",
        ];
        ui.set_timelapse_available(
            timelapse_keys
                .iter()
                .any(|key| self.status.contains_key(*key)),
        );
        if let Some(enabled) = timelapse_keys.iter().find_map(|key| {
            let object = self.object(key);
            ["enabled", "enable", "variable_enable"]
                .iter()
                .find_map(|field| {
                    object[field]
                        .as_bool()
                        .or_else(|| object[field].as_i64().map(|value| value != 0))
                })
        }) {
            ui.set_timelapse_enabled(enabled);
        }
    }

    fn open_print_lanes(&mut self, ui: &Moondash, tool: &str) {
        let lanes: Vec<Row> = self
            .status
            .iter()
            .filter_map(|(key, lane)| {
                let name = key.strip_prefix("AFC_stepper ")?;
                let detail = lane["material"].as_str().unwrap_or_default();
                Some(row(
                    name,
                    detail,
                    "apply-print-lane",
                    name,
                    tr(ui, "select"),
                ))
            })
            .collect();
        if !lanes.is_empty() {
            self.print_mapping = Some(tool.into());
            set_model(&ui.get_dialog_options(), &lanes, |model| {
                ui.set_dialog_options(model)
            });
            ui.set_print_lane_picker(true);
        }
    }

    fn browse_to(&mut self, ui: &Moondash, path: String) {
        if !ui.get_folder_picker() || self.invalid_folder_target(ui, &path) {
            return;
        }
        self.browsing_folder = path;
        self.browse_entries.clear();
        ui.set_folder_picker_loading(true);
        ui.set_folder_picker_at_root(self.browsing_folder.is_empty());
        ui.set_folder_picker_path(folder_label(ui, &self.browsing_folder).into());
        ui.set_folder_picker_error("".into());
        ui.set_folder_picker_rows(ModelRc::default());
        self.validate_picker(ui);
        if !self.try_call(
            ui,
            "server.files.get_directory",
            json!({"path":gcode_path(&self.browsing_folder)}),
            format!("browse:{}", self.browsing_folder),
        ) {
            ui.set_folder_picker_loading(false);
        }
    }

    fn invalid_folder_target(&self, ui: &Moondash, path: &str) -> bool {
        ui.get_dialog_folder()
            && (path == self.dialog.argument
                || path.starts_with(&format!("{}/", self.dialog.argument)))
    }

    fn move_target(&self, ui: &Moondash) -> Option<String> {
        let name = ui.get_move_name();
        if !valid_leaf(name.as_str()) || self.invalid_folder_target(ui, &self.destination_folder) {
            return None;
        }
        if !ui.get_dialog_folder()
            && !name.rsplit_once('.').is_some_and(|(_, extension)| {
                extension.eq_ignore_ascii_case("gcode") || extension.eq_ignore_ascii_case("gco")
            })
        {
            return None;
        }
        let target = join_path(&self.destination_folder, name.as_str());
        let prefix = format!("{target}/");
        if target == self.dialog.argument
            || print_files::files(&self.file_listing)
                .iter()
                .filter_map(print_files::filename)
                .any(|path| path == target || ui.get_dialog_folder() && path.starts_with(&prefix))
            || (ui.get_dialog_folder()
                && self.browsing_folder == self.destination_folder
                && self
                    .browse_entries
                    .iter()
                    .any(|entry| entry == name.as_str()))
        {
            return None;
        }
        Some(target)
    }

    fn validate_picker(&self, ui: &Moondash) {
        ui.set_move_valid(ui.get_dialog_mode() == "file-move" && self.move_target(ui).is_some());
        let name = ui.get_folder_name();
        ui.set_folder_create_valid(
            valid_leaf(name.as_str())
                && !self
                    .browse_entries
                    .iter()
                    .any(|entry| entry == name.as_str()),
        );
    }

    fn file_feedback(&self, ui: &Moondash, message: &'static str) {
        ui.set_file_feedback(message.into());
        let weak = ui.as_weak();
        Timer::single_shot(Duration::from_secs(4), move || {
            if let Some(ui) = weak.upgrade()
                && ui.get_file_feedback() == message
            {
                ui.set_file_feedback("".into());
            }
        });
    }

    fn file_operation(&self, ui: &Moondash, method: &'static str, params: Value, tag: &str) {
        if ui.get_file_busy() {
            return;
        }
        ui.set_dialog_detail("".into());
        ui.set_folder_picker_error("".into());
        if self.try_call(ui, method, params, tag) {
            ui.set_file_busy(true);
        }
    }

    fn filter_files(&mut self, ui: &Moondash, value: &str) {
        self.filter = value.into();
        ui.set_filter(value.into());
        ui.set_file_page(0);
        self.render_rows(ui);
    }

    fn dialog(
        &mut self,
        ui: &Moondash,
        title: &str,
        detail: &str,
        mode: &str,
        action: &str,
        argument: &str,
    ) {
        self.dialog = Dialog {
            action: action.into(),
            argument: argument.into(),
        };
        ui.set_dialog_title(title.into());
        ui.set_dialog_detail(detail.into());
        ui.set_dialog_value("".into());
        ui.set_dialog_image(Image::default());
        ui.set_dialog_folder(false);
        ui.set_dialog_time("".into());
        ui.set_dialog_weight("".into());
        ui.set_file_dialog_loading(false);
        ui.set_folder_picker(false);
        ui.set_print_lane_picker(false);
        ui.set_dialog_icon("".into());
        ui.set_dialog_unit("".into());
        ui.set_dialog_current("".into());
        ui.set_dialog_limit("".into());
        ui.set_dialog_secret(false);
        ui.set_keyboard_target(if mode == "text" {
            "dialog".into()
        } else {
            "".into()
        });
        ui.set_dialog_mode(mode.into());
    }

    fn show_color_picker(
        &mut self,
        ui: &Moondash,
        title: &str,
        action: &str,
        argument: &str,
        selected: u32,
        mut extras: Vec<u32>,
    ) {
        let theme_color = action == "theme-color";
        let colors = if theme_color {
            &theme::SWATCHES[4..]
        } else {
            &theme::SWATCHES[..]
        };
        let mut seen = HashSet::new();
        extras.retain(|color| seen.insert(*color));
        if !colors.contains(&selected) && !extras.contains(&selected) {
            extras.push(selected);
        }
        let swatch = |color: &u32| ColorSwatch {
            hex: format!("#{color:06X}").into(),
            fill: rgb(*color),
        };
        let extra_title = if theme_color {
            tr(ui, "default_colors")
        } else {
            tr(ui, "other_lane_colors")
        };
        ui.set_picker_swatches(ModelRc::new(VecModel::from(
            colors.iter().map(swatch).collect::<Vec<_>>(),
        )));
        ui.set_picker_extras(ModelRc::new(VecModel::from(
            extras.iter().map(swatch).collect::<Vec<_>>(),
        )));
        ui.set_picker_selected(rgb(selected));
        ui.set_picker_extras_title(extra_title.into());
        ui.set_picker_enabled(theme_color);
        self.dialog(ui, title, "", "color", action, argument);
    }

    fn action(&mut self, ui: &Moondash, action: &str, argument: &str) {
        match action {
            "dismiss" => ui.set_message("".into()),
            "dismiss-notification" => self.dismiss_notification(ui, argument),
            "reconnect-moonraker" if !self.online => self.endpoint.send_modify(|_| {}),
            "show-updates" => self.navigate(ui, "settings", "updates"),
            "refresh" => self.refresh(ui),
            "tool-mesh" | "tool-pid" | "tool-camera" => {
                let (view, title) = match action {
                    "tool-mesh" => ("mesh", tr(ui, "bed_mesh")),
                    "tool-pid" => ("pid", tr(ui, "pid_tuning")),
                    _ => ("camera", tr(ui, "webcam")),
                };
                self.tool_view = view.into();
                ui.set_tool_title(title.into());
                if view == "camera" {
                    self.call(ui, "server.webcams.list", json!({}), "webcams");
                }
                self.render_rows(ui);
            }
            "tools-home" => {
                self.tool_view.clear();
                self.webcam = None;
                ui.set_tool_title("".into());
                ui.set_tool_image(Image::default());
                self.render_rows(ui);
            }
            "power" => self.dialog(ui, tr(ui, "power_options"), "", "power", "", ""),
            "clear-message" => self.script(ui, "M117"),
            "clear-file" => self.script(ui, "SDCARD_RESET_FILE"),
            "reprint" => {
                let path = self.filename.clone();
                if !path.is_empty() {
                    self.action(ui, "file", &path);
                }
            }
            "show-objects" => self.navigate(ui, "tune", "objects"),
            "show-queue" => self.navigate(ui, "tune", "queue"),
            "show-afc" => self.navigate(ui, "tune", "afc"),
            "afc-edit" => {
                self.dialog(ui, argument, "", "menu", "", "");
                let options = [
                    (tr(ui, "material"), "afc-material", "printer-3d-nozzle"),
                    (tr(ui, "color"), "afc-color", "palette"),
                    (tr(ui, "weight"), "afc-weight", "scale"),
                    (tr(ui, "tool_mapping"), "afc-map", "source-branch"),
                ]
                .map(|(title, action, icon)| Row {
                    icon: icons::glyph(icon),
                    ..row(title, "", action, argument, "")
                });
                ui.set_dialog_options(ModelRc::new(VecModel::from(options.to_vec())));
            }
            "afc-unit" => {
                self.afc_unit = argument.into();
                self.update_afc(ui);
            }
            "file-sort" => {
                self.file_sort = argument.parse().unwrap_or_default();
                ui.set_file_sort(i32::from(self.file_sort));
                ui.set_file_page(0);
                self.render_rows(ui);
            }
            "file-filter" => ui.set_keyboard_target("filter".into()),
            "file-context" => {
                use slint::Model;
                let model = ui.get_rows();
                if let Some(entry) = model
                    .iter()
                    .find(|entry| entry.argument == argument && !entry.alternate.is_empty())
                {
                    self.dialog(
                        ui,
                        entry.title.as_str(),
                        "",
                        "file-context",
                        entry.alternate.as_str(),
                        argument,
                    );
                    ui.set_dialog_folder(entry.action == "folder");
                    ui.set_dialog_image(entry.preview);
                    if entry.action == "file" {
                        self.show_file_details(ui, argument);
                    }
                }
            }
            "delete-selected" => {
                ui.set_dialog_detail("".into());
                ui.set_dialog_mode("file-delete".into());
            }
            "apply-delete" => {
                let method = if ui.get_dialog_folder() {
                    "server.files.delete_directory"
                } else {
                    "server.files.delete_file"
                };
                self.file_operation(
                    ui,
                    method,
                    json!({"path":gcode_path(&self.dialog.argument)}),
                    "file-deleted",
                );
            }
            "file-context-back" => {
                ui.set_dialog_detail("".into());
                ui.set_dialog_mode("file-context".into());
            }
            "move-selected" => {
                let path = self.dialog.argument.as_str();
                self.destination_folder = parent_dir(path).to_owned();
                ui.set_move_destination(folder_label(ui, &self.destination_folder).into());
                ui.set_move_name(print_files::leaf(path).into());
                ui.set_dialog_detail("".into());
                ui.set_dialog_mode("file-move".into());
                self.validate_picker(ui);
            }
            "browse-destination" => {
                ui.set_folder_picker(true);
                ui.set_folder_create(false);
                self.browse_to(ui, self.destination_folder.clone());
            }
            "close-folder-picker" => ui.set_folder_picker(false),
            "browse-folder" => self.browse_to(ui, argument.to_owned()),
            "browse-folder-up" => self.browse_to(ui, parent_dir(&self.browsing_folder).to_owned()),
            "select-this-folder" => {
                self.destination_folder.clone_from(&self.browsing_folder);
                ui.set_move_destination(folder_label(ui, &self.destination_folder).into());
                ui.set_folder_picker(false);
                self.validate_picker(ui);
            }
            "start-new-folder" => {
                ui.set_folder_name("".into());
                ui.set_folder_create(true);
                ui.set_keyboard_target("folder-name".into());
                self.validate_picker(ui);
            }
            "cancel-new-folder" => ui.set_folder_create(false),
            "new-folder-keyboard" => ui.set_keyboard_target("folder-name".into()),
            "filename-keyboard" => ui.set_keyboard_target("move-name".into()),
            "apply-new-folder" if ui.get_folder_create_valid() => {
                let name = ui.get_folder_name();
                if valid_leaf(name.as_str())
                    && !self
                        .browse_entries
                        .iter()
                        .any(|existing| existing == name.as_str())
                {
                    let path = join_path(&self.browsing_folder, name.as_str());
                    self.file_operation(
                        ui,
                        "server.files.post_directory",
                        json!({"path":gcode_path(&path)}),
                        &format!("folder-created:{path}"),
                    );
                }
            }
            "apply-move" => {
                if let Some(path) = self.move_target(ui) {
                    self.file_operation(
                        ui,
                        "server.files.move",
                        json!({
                            "source": gcode_path(&self.dialog.argument), "dest": gcode_path(&path)
                        }),
                        "file-moved",
                    );
                }
            }
            "print-file"
                if ui.get_dialog_mode() == "print"
                    && ui.get_ready()
                    && !ui.get_printing()
                    && !ui.get_paused() =>
            {
                self.file_operation(
                    ui,
                    "printer.print.start",
                    json!({"filename": self.dialog.argument}),
                    "file-printed",
                );
            }
            "queue-file" if ui.get_dialog_mode() == "print" => {
                self.file_operation(
                    ui,
                    "server.job_queue.post_job",
                    json!({"filenames": [self.dialog.argument]}),
                    "file-queued",
                );
            }
            "print-tool" => self.open_print_lanes(ui, argument),
            "close-print-lanes" => {
                self.print_mapping = None;
                ui.set_print_lane_picker(false);
            }
            "apply-print-lane" => {
                if let Some(tool) = self
                    .print_mapping
                    .take()
                    .filter(|_| self.status.contains_key(&format!("AFC_stepper {argument}")))
                {
                    self.file_operation(ui, "printer.gcode.script", json!({"script": format!("SET_MAP LANE={} MAP={}", gcode_arg(argument), gcode_arg(&tool))}), &format!("print-map:{tool}:{argument}"));
                    ui.set_print_lane_picker(false);
                }
            }
            "print-timelapse" if ui.get_timelapse_available() => {
                let enabled = !ui.get_timelapse_enabled();
                self.file_operation(
                    ui,
                    "machine.timelapse.post_settings",
                    json!({"enabled":enabled}),
                    if enabled {
                        "timelapse:on"
                    } else {
                        "timelapse:off"
                    },
                );
            }
            "config-language" if matches!(argument, "auto" | "en" | "de") => {
                let value = if argument == "auto" { "null" } else { argument };
                self.change_setting(ui, "system.language", value);
            }
            "config-toggle" => {
                if let Some((section, key)) = argument.split_once('.') {
                    let value = !self.config[section][key].as_bool().unwrap_or_default();
                    self.change_setting(ui, argument, if value { "true" } else { "false" });
                }
            }
            "config-profile" if matches!(argument, "auto" | "small" | "normal" | "highres") => {
                self.change_setting(ui, "system.display_profile", argument);
            }
            "config-timeout" if matches!(argument, "60" | "300" | "900" | "1800" | "3600") => {
                self.change_setting(ui, "system.idle_timeout", argument);
            }
            "config-color" | "config-secondary" => {
                let key = if action == "config-secondary" {
                    "secondary"
                } else {
                    "primary"
                };
                let defaults = theme::theme_defaults(ui.get_dark());
                let default = defaults[usize::from(key == "secondary")];
                let selected = self.config["styling"][key]
                    .as_str()
                    .and_then(theme::parse_rgb)
                    .unwrap_or(default);
                let title = if key == "primary" {
                    tr(ui, "choose_primary_color")
                } else {
                    tr(ui, "choose_secondary_color")
                };
                self.show_color_picker(ui, title, "theme-color", key, selected, defaults.to_vec());
            }
            "config-edit" if argument == "websocket.ip" => {
                let initial = self.endpoint.borrow().clone();
                self.dialog(
                    ui,
                    tr(ui, "moonraker_ip"),
                    tr(ui, "host_and_port_or_url"),
                    "text",
                    action,
                    argument,
                );
                ui.set_dialog_value(initial.into());
            }
            "edit-shortcut" => {
                self.dialog(
                    ui,
                    argument,
                    tr(
                        ui,
                        "macro_to_run_edit_the_configuration_file_to_set_advanced_options",
                    ),
                    "text",
                    action,
                    argument,
                );
                if let Some(value) = array(&self.config["shortcutbuttons"])
                    .iter()
                    .find(|item| item["name"].as_str() == Some(argument))
                    .and_then(|item| item["macro_inactive"].as_str())
                {
                    ui.set_dialog_value(value.into());
                }
            }
            "add-shortcut" => self.dialog(
                ui,
                tr(ui, "add_shortcut"),
                tr(ui, "shortcut_name"),
                "text",
                action,
                "",
            ),
            "emergency" => self.call(ui, "printer.emergency_stop", json!({}), "emergency"),
            "pause" => self.call(ui, "printer.print.pause", json!({}), "pause"),
            "resume" => self.call(ui, "printer.print.resume", json!({}), "resume"),
            "cancel" => self.dialog(
                ui,
                tr(ui, "cancel_print_question"),
                "",
                "confirm",
                action,
                argument,
            ),
            "folder" | "up" => {
                self.folder = if action == "up" {
                    parent_dir(&self.folder).to_owned()
                } else {
                    argument.to_owned()
                };
                ui.set_file_page(0);
                self.render_rows(ui);
            }
            "file" => {
                self.dialog(
                    ui,
                    print_files::display_name(argument),
                    "",
                    "print",
                    action,
                    argument,
                );
                self.show_file_details(ui, argument);
            }
            action if action.starts_with("queue-remove:") => {
                self.dialog(
                    ui,
                    tr(ui, "remove_queued_job_question"),
                    argument,
                    "confirm",
                    "queue-remove",
                    &action["queue-remove:".len()..],
                );
            }
            "queue-start" => self.dialog(
                ui,
                tr(ui, "start_job_queue_question"),
                "",
                "confirm",
                action,
                argument,
            ),
            "queue-pause" => self.call(ui, "server.job_queue.pause", json!({}), "queue"),
            "home" => self.motion(ui, format!("G28 {argument}")),
            "set-step" => {
                if let Ok(distance) = argument.parse() {
                    self.jog_step = distance;
                    ui.set_jog_step(distance as f32);
                }
            }
            "load-filament" => self.motion(ui, "LOAD_FILAMENT"),
            "unload-filament" => self.motion(ui, "UNLOAD_FILAMENT"),
            "toggle-filament-sensor" => {
                if let Some(name) = filament_sensor_name(argument)
                    && let Some(data) = self.status.get(argument)
                {
                    let enable = u8::from(!filament_sensor_enabled(data));
                    self.submit_control(
                        ui,
                        format!(
                            "SET_FILAMENT_SENSOR SENSOR={} ENABLE={enable}",
                            gcode_arg(name)
                        ),
                    );
                }
            }
            "z-tilt" => self.motion(ui, "Z_TILT_ADJUST"),
            "qgl" => self.motion(
                ui,
                if self.status.contains_key("gcode_macro QGL") {
                    "QGL"
                } else {
                    "QUAD_GANTRY_LEVEL"
                },
            ),
            "set-flow" | "set-advance" | "set-smooth" => {
                if let Ok(value) = argument.trim().replace(',', ".").parse::<f64>()
                    && value.is_finite()
                {
                    let command = match action {
                        "set-flow" => format!("M221 S{:.0}", value.clamp(1., 300.)),
                        "set-advance" => {
                            format!("SET_PRESSURE_ADVANCE ADVANCE={:.3}", value.clamp(0., 1.))
                        }
                        _ => format!(
                            "SET_PRESSURE_ADVANCE SMOOTH_TIME={:.3}",
                            value.clamp(0., 0.2)
                        ),
                    };
                    self.script(ui, command);
                }
            }
            "disable-motors" => self.dialog(
                ui,
                tr(ui, "disable_motors_question"),
                "",
                "confirm",
                action,
                argument,
            ),
            "jog-plus" | "jog-minus" => {
                let distance = if action == "jog-plus" {
                    self.jog_step
                } else {
                    -self.jog_step
                };
                self.motion(ui, format!("SAVE_GCODE_STATE NAME=moondash_jog\nG91\nG0 {argument}{distance} F{}\nRESTORE_GCODE_STATE NAME=moondash_jog", if argument == "Z" { 600 } else { 3600 }));
            }
            "z-plus" | "z-minus" => self.script(
                ui,
                format!(
                    "SET_GCODE_OFFSET Z_ADJUST={} MOVE=1",
                    if action == "z-plus" {
                        "0.025"
                    } else {
                        "-0.025"
                    }
                ),
            ),
            "heater" | "temperature-fan" => {
                let (title, icon) = if action == "temperature-fan" {
                    (
                        argument
                            .strip_prefix("temperature_fan ")
                            .unwrap_or(argument),
                        "fan",
                    )
                } else if argument.starts_with("extruder") {
                    (argument, "printer-3d-nozzle-heat")
                } else {
                    (
                        argument.strip_prefix("heater_generic ").unwrap_or(argument),
                        "radiator",
                    )
                };
                self.dialog(ui, title, "", "temperature", action, argument);
                ui.set_dialog_icon(icons::glyph(icon));
                self.update_temperature_dialog(ui, argument);
            }
            "speed" => {
                self.edited_speed_percent = None;
                self.initial_speed_acceleration = self.speed_acceleration();
                self.dialog(ui, "", "", "speed", action, argument);
                ui.set_speed_target(self.speed_percent().unwrap_or(100));
                ui.set_speed_acceleration(
                    self.initial_speed_acceleration
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                        .into(),
                );
            }
            "fan" => {
                let name = argument.strip_prefix("fan_generic ").unwrap_or(argument);
                let title = name.trim_matches('_').replace('_', " ");
                let target = number(&self.object(argument)["speed"]).unwrap_or_default() * 100.;
                ui.set_fan_target(target.round().clamp(0., 100.) as i32);
                self.dialog(ui, &title, "", "fan", action, argument);
            }
            "step" | "extrude" | "retract" | "flow" | "pid" => {
                let (title, detail, initial) = match action {
                    "step" => (tr(ui, "jog_distance"), tr(ui, "millimetres"), self.jog_step),
                    "extrude" => (tr(ui, "extrude"), tr(ui, "millimetres"), 5.),
                    "retract" => (tr(ui, "retract"), tr(ui, "millimetres"), 5.),
                    "flow" => (
                        tr(ui, "extrusion_factor"),
                        tr(ui, "percent"),
                        number(&self.object("gcode_move")["extrude_factor"]).unwrap_or(1.) * 100.,
                    ),
                    _ => (
                        tr(ui, "pid_calibration"),
                        tr(
                            ui,
                            "target_temperature_in_c_klipper_will_heat_the_selected_heater",
                        ),
                        if argument == "heater_bed" { 60. } else { 200. },
                    ),
                };
                self.dialog(ui, title, detail, "number", action, argument);
                let (label, icon, unit, limit, current) = match action {
                    "pid" => {
                        let limit = self.heater_limit(argument);
                        let current = number(&self.object(argument)["temperature"])
                            .map(|temp| format!("{temp:.0}°C"))
                            .unwrap_or_else(|| "--°C".into());
                        (
                            title,
                            if argument == "heater_bed" {
                                "radiator"
                            } else {
                                "printer-3d-nozzle-heat"
                            },
                            "°C",
                            format!("{limit:.0}°C"),
                            current,
                        )
                    }
                    "flow" => (
                        title,
                        "speedometer-medium",
                        "%",
                        String::new(),
                        format!("{initial:.0}%"),
                    ),
                    _ => (title, "printer-3d", " mm", String::new(), String::new()),
                };
                ui.set_dialog_title(label.into());
                ui.set_dialog_icon(icons::glyph(icon));
                ui.set_dialog_unit(unit.into());
                ui.set_dialog_current(current.into());
                ui.set_dialog_limit(limit.into());

                ui.set_dialog_value(
                    format!("{initial:.2}")
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .into(),
                );
            }
            "macro" => self.dialog(
                ui,
                tr(ui, "run_macro_question"),
                argument,
                "confirm",
                action,
                argument,
            ),
            "shortcut" => {
                if let Some(shortcut) = array(&self.config["shortcutbuttons"])
                    .iter()
                    .find(|item| item["name"].as_str() == Some(argument))
                {
                    let key = if shortcut_active(&self.status, shortcut) {
                        "macro_active"
                    } else {
                        "macro_inactive"
                    };
                    if let Some(script) = shortcut[key]
                        .as_str()
                        .filter(|script| !script.is_empty())
                        .or(shortcut["macro_inactive"].as_str())
                    {
                        self.script(ui, script);
                    }
                }
            }
            "afc-load" | "afc-unload" | "exclude" => self.dialog(
                ui,
                match action {
                    "afc-load" => tr(ui, "load_lane_question"),
                    "afc-unload" => tr(ui, "unload_lane_question"),
                    _ => tr(ui, "skip_object_question"),
                },
                argument,
                "confirm",
                action,
                argument,
            ),
            "afc-color" => {
                let lane = format!("AFC_stepper {argument}");
                if let Some(data) = self.status.get(&lane) {
                    let selected = data["color"]
                        .as_str()
                        .and_then(theme::parse_rgb)
                        .unwrap_or(0x434343);
                    let extras = self
                        .status
                        .iter()
                        .filter(|(key, _)| key.starts_with("AFC_stepper ") && **key != lane)
                        .filter_map(|(_, data)| data["color"].as_str().and_then(theme::parse_rgb))
                        .filter(|color| *color != 0x434343)
                        .collect();
                    let title = tr(ui, "choose_color");
                    self.show_color_picker(ui, title, action, argument, selected, extras);
                }
            }
            "afc-material" | "afc-map" | "afc-runout" | "afc-weight" => self.dialog(
                ui,
                match action {
                    "afc-material" => tr(ui, "material"),
                    "afc-map" => tr(ui, "tool_mapping"),
                    "afc-weight" => tr(ui, "weight_g"),
                    _ => tr(ui, "runout_destination"),
                },
                argument,
                "text",
                action,
                argument,
            ),
            "afc-reset" => self.script(ui, "AFC_CLEAR_MESSAGE"),
            "mesh-info" => {
                let mesh = self.object("bed_mesh");
                let matrix = array(&mesh["mesh_matrix"])
                    .iter()
                    .map(|line| {
                        array(line)
                            .iter()
                            .filter_map(number)
                            .map(|value| format!("{value:+.3}"))
                            .collect::<Vec<_>>()
                            .join("  ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.dialog(
                    ui,
                    tr(ui, "bed_mesh"),
                    if matrix.is_empty() {
                        tr(ui, "no_active_mesh")
                    } else {
                        &matrix
                    },
                    "info",
                    "",
                    "",
                );
            }
            "mesh-load" => {
                self.script(ui, format!("BED_MESH_PROFILE LOAD={}", gcode_arg(argument)))
            }
            "mesh-calibrate" | "mesh-clear" | "mesh-remove" | "save-printer"
            | "restart-klipper" | "restart-firmware" | "reboot" | "shutdown"
            | "restart-service" => {
                let title = match action {
                    "mesh-calibrate" => tr(ui, "calibrate_bed_mesh_question"),
                    "mesh-clear" => tr(ui, "clear_bed_mesh_question"),
                    "mesh-remove" => tr(ui, "remove_profile_question"),
                    "save-printer" => tr(ui, "save_configuration_and_restart_klipper_question"),
                    "restart-klipper" => tr(ui, "restart_klipper_question"),
                    "restart-firmware" => tr(ui, "restart_firmware_question"),
                    "reboot" => tr(ui, "reboot_host_question"),
                    "shutdown" => tr(ui, "shut_down_host_question"),
                    _ => tr(ui, "restart_moondash_service_question"),
                };
                self.dialog(ui, title, argument, "confirm", action, argument);
            }
            "storage" => self.call(
                ui,
                "server.files.get_directory",
                json!({"path": "gcodes", "extended": true}),
                "storage",
            ),
            "history" => self.call(ui, "server.history.totals", json!({}), "history"),
            "webcam" if self.webcam.is_some() => {
                self.webcam = None;
                ui.set_tool_image(Image::default());
            }
            "webcam" => self.call(ui, "server.webcams.list", json!({}), "webcams"),
            "update-refresh" => self.call(
                ui,
                "machine.update.status",
                json!({"refresh": true}),
                "updates",
            ),
            "update-all" | "update" if ui.get_printing() || ui.get_paused() => {
                ui.set_message(tr(ui, "finish_the_print_before_installing_updates").into());
            }
            "update-all" | "update" => self.dialog(
                ui,
                tr(ui, "install_updates_question"),
                argument,
                "confirm",
                action,
                argument,
            ),
            "wifi-on" | "wifi-off" | "wifi-scan" | "wifi-forget" | "wired-on" | "wired-off" => {
                self.network_action(ui, action, argument, "");
            }
            "wifi-saved" | "wifi-hidden"
                if matches!(ui.get_network_job().as_str(), "" | "network") =>
            {
                ui.set_network_error("".into());
                ui.set_network_ssid("".into());
                ui.set_network_password("".into());
                ui.set_network_dialog(
                    if action == "wifi-saved" {
                        "saved"
                    } else {
                        "hidden"
                    }
                    .into(),
                );
            }
            "wifi-connect" if ui.get_network_job().is_empty() => {
                let wifi = &self.network["wifi"];
                let saved = array(&wifi["savedNetworks"])
                    .iter()
                    .any(|item| item["ssid"].as_str() == Some(argument));
                let secured = array(&wifi["scannedNetworks"])
                    .iter()
                    .find(|item| item["ssid"].as_str() == Some(argument))
                    .is_some_and(|item| item["secured"].as_bool() == Some(true));
                if secured && !saved {
                    ui.set_network_error("".into());
                    ui.set_network_ssid(argument.into());
                    ui.set_network_password("".into());
                    ui.set_network_password_title(
                        tr(ui, "connect_to_ssid").replace("{ssid}", argument).into(),
                    );
                    ui.set_network_dialog("password".into());
                } else {
                    self.network_action(ui, action, argument, "");
                }
            }
            "wifi-submit" => {
                let ssid = ui.get_network_ssid();
                let password = ui.get_network_password();
                match ui.get_network_dialog().as_str() {
                    "hidden" if !ssid.is_empty() => {
                        self.network_action(
                            ui,
                            "wifi-hidden-password",
                            ssid.as_str(),
                            password.as_str(),
                        );
                    }
                    "password" if !ssid.is_empty() && !password.is_empty() => {
                        self.network_action(ui, "wifi-connect", ssid.as_str(), password.as_str());
                    }
                    _ => {}
                }
            }
            "wifi-close" => {
                let job = ui.get_network_job();
                if job.is_empty() || job == "network" || job == "wifi-scan" {
                    let back = if ui.get_network_dialog() == "password" {
                        "scan"
                    } else {
                        ""
                    };
                    ui.set_network_dialog(back.into());
                    ui.set_network_error("".into());
                    ui.set_network_password("".into());
                    ui.set_network_ssid("".into());
                    ui.set_keyboard_target("".into());
                }
            }
            "sleep" => {
                if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                    self.manual_sleep.store(true, Ordering::Relaxed);
                } else {
                    ui.set_message(tr(ui, "display_power_control_requires_wayland").into());
                }
            }
            _ => {}
        }
    }

    fn apply_picked_color(&mut self, ui: &Moondash, action: &str, argument: &str, value: &str) {
        let Some(color) = theme::parse_rgb(value) else {
            return;
        };
        if action == "theme-color" && matches!(argument, "primary" | "secondary") {
            let hex = format!("#{color:06X}");
            match self.save_setting(ui, &format!("styling.{argument}"), &hex) {
                Ok(()) => ui.set_dialog_mode("".into()),
                Err(error) => ui.set_dialog_detail(error.into()),
            }
        } else if action == "afc-color"
            && self.status.contains_key(&format!("AFC_stepper {argument}"))
        {
            self.submit_printer_setting(
                ui,
                format!("SET_COLOR LANE={} COLOR={color:06X}", gcode_arg(argument)),
            );
        }
    }

    fn submit(&mut self, ui: &Moondash, input: &str, _queue: bool) {
        let action = self.dialog.action.clone();
        let argument = self.dialog.argument.clone();
        let value = input.trim();

        if ui.get_dialog_mode() == "speed" {
            self.save_speed_settings(ui);
            return;
        }
        if ui.get_dialog_mode() == "temperature" {
            self.set_temperature(ui, &argument, value);
            return;
        }
        if ui.get_dialog_mode() == "fan" {
            self.set_fan(ui, &argument, value);
            return;
        }
        if ui.get_dialog_mode() == "number" {
            let limit = match action.as_str() {
                "step" | "extrude" | "retract" => 100.,
                "flow" => 500.,
                "pid" => self.heater_limit(&argument),
                _ => return,
            };
            let minimum = match action.as_str() {
                "step" | "extrude" | "retract" | "flow" => 0.01,
                "pid" => 1.,
                _ => 0.,
            };
            let Some(number) = value
                .parse::<f64>()
                .ok()
                .filter(|number| (minimum..=limit).contains(number))
            else {
                let message = tr(ui, "number_range")
                    .replace("{minimum}", &minimum.to_string())
                    .replace("{maximum}", &limit.to_string());
                ui.set_dialog_detail(message.into());
                return;
            };
            self.submit_number(ui, &action, &argument, number);
            ui.set_dialog_mode("".into());
            return;
        }

        match action.as_str() {
            "file-filter" => {
                self.filter = value.into();
                ui.set_filter(value.into());
                ui.set_file_page(0);
                self.render_rows(ui);
            }
            "theme-color" | "afc-color" => {
                self.apply_picked_color(ui, &action, &argument, value);
                return;
            }
            "config-edit" if argument == "websocket.ip" => {
                if moonraker::endpoint(value).is_err() || value.contains(['\n', '\r']) {
                    ui.set_dialog_detail(tr(ui, "enter_a_valid_value").into());
                    return;
                }
                self.change_setting(ui, &argument, value);
            }
            "add-shortcut" => {
                if value.is_empty()
                    || value.contains(['[', ']', '\n', '\r'])
                    || self.config.get(format!("shortcutbutton {value}")).is_some()
                {
                    ui.set_dialog_detail(tr(ui, "enter_a_unique_shortcut_name").into());
                    return;
                }
                self.dialog(
                    ui,
                    value,
                    tr(ui, "macro_to_run"),
                    "text",
                    "new-shortcut",
                    value,
                );
                return;
            }
            "edit-shortcut" | "new-shortcut" => {
                if value.is_empty() || value.contains(['\n', '\r']) {
                    ui.set_dialog_detail(tr(ui, "enter_a_single_line_g_code_command").into());
                    return;
                }
                let section = format!("shortcutbutton {argument}");
                let text = config::with_value(
                    ui.get_config_text().as_str(),
                    &section,
                    "macro_inactive",
                    value,
                );
                let text = if action == "new-shortcut" {
                    config::with_value(&text, &section, "icon", "mdi-lightning-bolt")
                } else {
                    text
                };
                self.store_config(ui, &text);
            }
            "cancel" => self.call(ui, "printer.print.cancel", json!({}), "cancel"),
            "queue-start" => self.call(ui, "server.job_queue.start", json!({}), "queue"),
            "queue-remove" => self.call(
                ui,
                "server.job_queue.delete_job",
                json!({"job_ids": [argument]}),
                "queue",
            ),
            "disable-motors" => self.motion(ui, "M84"),
            "macro" => self.script(ui, argument),
            "exclude" => self.script(ui, format!("EXCLUDE_OBJECT NAME={}", gcode_arg(&argument))),
            "afc-load" => self.script(ui, format!("TOOL_LOAD LANE={}", gcode_arg(&argument))),
            "afc-unload" => self.script(ui, format!("LANE_UNLOAD LANE={}", gcode_arg(&argument))),
            "afc-material" | "afc-map" | "afc-runout" | "afc-weight" => {
                if value.is_empty() || value.contains(['\n', '\r']) {
                    ui.set_dialog_detail(tr(ui, "enter_a_value").into());
                    return;
                }
                let parameter = match action.as_str() {
                    "afc-material" => "MATERIAL",
                    "afc-map" => "MAP",
                    "afc-weight" => "WEIGHT",
                    _ => "RUNOUT",
                };
                self.script(
                    ui,
                    format!(
                        "SET_{parameter} LANE={} {parameter}={}",
                        gcode_arg(&argument),
                        gcode_arg(value)
                    ),
                );
            }
            "mesh-calibrate" => self.script(ui, "BED_MESH_CALIBRATE"),
            "mesh-clear" => self.script(ui, "BED_MESH_CLEAR"),
            "mesh-remove" => self.script(
                ui,
                format!("BED_MESH_PROFILE REMOVE={}", gcode_arg(&argument)),
            ),
            "save-printer" => self.script(ui, "SAVE_CONFIG"),
            "restart-klipper" => self.call(ui, "printer.restart", json!({}), "restart"),
            "restart-firmware" => self.call(ui, "printer.firmware_restart", json!({}), "restart"),
            "reboot" => self.call(ui, "machine.reboot", json!({}), "reboot"),
            "shutdown" => self.call(ui, "machine.shutdown", json!({}), "shutdown"),
            "restart-service" => {
                if let Err(error) = version::restart_after_package_update() {
                    ui.set_message(error.into());
                }
            }
            "update" => self.call(
                ui,
                "machine.update.upgrade",
                json!({"name": argument}),
                "update-finished",
            ),
            "update-all" => self.call(ui, "machine.update.full", json!({}), "update-finished"),
            _ => {}
        }
        ui.set_dialog_mode("".into());
    }

    fn submit_number(&mut self, ui: &Moondash, action: &str, argument: &str, value: f64) {
        let name = argument.split_once(' ').map_or(argument, |(_, name)| name);
        let script = match action {
            "step" => {
                self.jog_step = value;
                self.render_rows(ui);
                return;
            }
            "extrude" | "retract" => {
                let movement = if action == "extrude" { value } else { -value };
                format!(
                    "SAVE_GCODE_STATE NAME=moondash_extrude\nM83\nG1 E{movement} F240\nRESTORE_GCODE_STATE NAME=moondash_extrude"
                )
            }
            "flow" => format!("M221 S{value}"),
            "pid" => format!(
                "PID_CALIBRATE HEATER={} TARGET={}",
                gcode_arg(name),
                value.round()
            ),
            _ => return,
        };
        self.script(ui, script);
    }

    fn persist_config(&mut self, ui: &Moondash, text: &str) -> Result<(), String> {
        config::parse(text)?;
        save_config(&self.config_path, text).map_err(|error| error.to_string())?;
        self.load_config(ui, true);
        Ok(())
    }

    fn store_config(&mut self, ui: &Moondash, text: &str) {
        if let Err(error) = self.persist_config(ui, text) {
            ui.set_message(error.into());
        }
    }

    fn save_setting(&mut self, ui: &Moondash, path: &str, value: &str) -> Result<(), String> {
        let (section, key) = path.split_once('.').ok_or(tr(ui, "invalid_setting_path"))?;
        let existing = match fs::read_to_string(&self.config_path) {
            Ok(text) => text.into(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => ui.get_config_text(),
            Err(error) => return Err(error.to_string()),
        };
        let text = config::with_value(existing.as_str(), section, key, value);
        self.persist_config(ui, &text)
    }

    fn change_setting(&mut self, ui: &Moondash, path: &str, value: &str) {
        if let Err(error) = self.save_setting(ui, path, value) {
            ui.set_message(error.into());
        }
    }

    fn reply(&mut self, ui: &Moondash, tag: &str, reply: Result<Value, String>) {
        if let Some(action) = tag.strip_prefix("network:") {
            self.network_reply(ui, action, reply);
            return;
        }

        match tag {
            "notification-dismissals" => {
                if let Ok(result) = reply
                    && let Some(dismissals) = result["value"].as_object()
                {
                    self.dismissed_notifications.extend(
                        dismissals
                            .iter()
                            .filter(|(id, value)| {
                                value.as_bool() == Some(true) && is_persistent_warning(id)
                            })
                            .map(|(id, _)| id.clone()),
                    );
                    self.update_notifications(ui);
                }
                return;
            }
            "notification-server-info" => {
                if let Ok(result) = reply {
                    self.moonraker_warnings = result["warnings"].clone();
                    self.update_notifications(ui);
                }
                return;
            }
            "notification-updates" => {
                if let Ok(result) = reply {
                    self.set_updates(ui, result);
                }
                return;
            }
            _ => {}
        }
        if tag == "printer-setting" {
            ui.set_printer_setting_busy(false);
            match reply {
                Ok(_) => ui.set_dialog_mode("".into()),
                Err(error) => ui.set_dialog_detail(error.into()),
            }
            return;
        }
        if tag == "motion" {
            self.motion_busy = false;
            ui.set_motion_busy(false);
        }
        if tag == "file-list" {
            ui.set_file_loading(false);
        }
        let mutation = matches!(
            tag,
            "file-deleted" | "file-moved" | "file-printed" | "file-queued"
        ) || tag.starts_with("folder-created:")
            || tag.starts_with("print-map:")
            || tag.starts_with("timelapse:");
        if mutation {
            ui.set_file_busy(false);
        }
        if let Some(path) = tag.strip_prefix("browse:") {
            if ui.get_folder_picker() && path == self.browsing_folder {
                ui.set_folder_picker_loading(false);
                match reply {
                    Ok(listing) => {
                        self.browse_entries = folder_entries(&listing);
                        let folders: Vec<_> = self
                            .browse_entries
                            .iter()
                            .map(|name| {
                                let path = join_path(&self.browsing_folder, name);
                                Row {
                                    active: self.invalid_folder_target(ui, &path),
                                    ..row(name.as_str(), "", "browse-folder", &path, "")
                                }
                            })
                            .collect();
                        set_model(&ui.get_folder_picker_rows(), &folders, |model| {
                            ui.set_folder_picker_rows(model)
                        });
                        self.validate_picker(ui);
                    }
                    Err(error) => ui.set_folder_picker_error(error.into()),
                }
            }
            return;
        }
        if mutation && let Err(error) = &reply {
            if ui.get_folder_picker() {
                ui.set_folder_picker_error(error.as_str().into());
            } else {
                ui.set_dialog_detail(error.as_str().into());
            }
            return;
        }

        if reply.is_err()
            && let Some(path) = tag.strip_prefix("card:")
        {
            self.file_details.insert(path.into(), Value::Null);
            self.call(
                ui,
                "server.files.thumbnails",
                json!({"filename": path}),
                format!("thumb:card:{path}"),
            );
            if self.page == "files" {
                self.render_rows(ui);
            }
            return;
        }
        let result = match reply {
            Ok(result) => result,
            Err(error) => {
                if let Some(path) = tag.strip_prefix("file:") {
                    if self.dialog.argument == path
                        && (ui.get_dialog_mode().starts_with("file-")
                            || ui.get_dialog_mode() == "print")
                    {
                        ui.set_file_dialog_loading(false);
                        self.call(
                            ui,
                            "server.files.thumbnails",
                            json!({"filename": path}),
                            format!("thumb:file:{path}"),
                        );
                    }
                } else if !tag.starts_with("preview:")
                    && !tag.starts_with("thumb:")
                    && tag != "file-history"
                    && !(tag == "queue" && self.page == "home")
                {
                    ui.set_message(error.into());
                }
                return;
            }
        };
        if let Some(path) = tag.strip_prefix("card:") {
            self.thumbnail(tag, path, &result);
            if array(&result["thumbnails"]).is_empty() {
                self.call(
                    ui,
                    "server.files.thumbnails",
                    json!({"filename": path}),
                    format!("thumb:card:{path}"),
                );
            }
            self.file_details.insert(path.to_owned(), result);
            if self.page == "files" {
                self.render_rows(ui);
            }
            return;
        }
        if let Some(tag) = tag.strip_prefix("thumb:")
            && let Some((_, path)) = tag.split_once(':')
        {
            self.thumbnail(tag, path, &json!({"thumbnails": result}));
            return;
        }
        if let Some(path) = tag.strip_prefix("preview:") {
            if self.filename == path {
                self.thumbnail(tag, path, &result);
            }
            return;
        }
        if let Some(path) = tag.strip_prefix("file:") {
            if self.dialog.argument == path
                && (ui.get_dialog_mode() == "print" || ui.get_dialog_mode().starts_with("file-"))
            {
                self.metadata = result;
                self.update_print_details(ui);
                ui.set_file_dialog_loading(false);
                self.thumbnail(tag, path, &self.metadata);
                if array(&self.metadata["thumbnails"]).is_empty() {
                    self.call(
                        ui,
                        "server.files.thumbnails",
                        json!({"filename":path}),
                        format!("thumb:file:{path}"),
                    );
                }
            }
            return;
        }

        if let Some(path) = tag.strip_prefix("folder-created:") {
            ui.set_folder_create(false);
            ui.set_keyboard_target("".into());
            self.file_feedback(ui, tr(ui, "folder_created"));
            self.browse_to(ui, path.into());
            return;
        }
        if let Some((tool, selected)) = tag
            .strip_prefix("print-map:")
            .and_then(|tag| tag.split_once(':'))
        {
            for (key, lane) in &mut self.status {
                if let Some(name) = key.strip_prefix("AFC_stepper ")
                    && let Some(lane) = lane.as_object_mut()
                {
                    if name == selected {
                        lane.insert("map".into(), json!(tool));
                    } else if lane.get("map").and_then(Value::as_str) == Some(tool) {
                        lane.insert("map".into(), json!("NONE"));
                    }
                }
            }
            self.update_print_details(ui);
            return;
        }
        if let Some(value) = tag.strip_prefix("timelapse:") {
            let enabled = value == "on";
            if let Some(settings) = self
                .status
                .get_mut("timelapse_settings")
                .and_then(Value::as_object_mut)
            {
                settings.insert("enabled".into(), json!(enabled));
            } else {
                self.status
                    .insert("timelapse_settings".into(), json!({"enabled":enabled}));
            }
            ui.set_timelapse_enabled(enabled);
            return;
        }

        match tag {
            "file-deleted" | "file-moved" => {
                ui.set_dialog_mode("".into());
                let message = if tag == "file-deleted" {
                    tr(ui, "file_deleted_successfully")
                } else {
                    tr(ui, "file_moved_successfully")
                };
                self.file_feedback(ui, message);
                self.refresh(ui);
            }
            "file-printed" => ui.set_dialog_mode("".into()),
            "file-queued" => {
                ui.set_dialog_mode("".into());
                if result["queued_jobs"].is_array() {
                    self.queue = result;
                    self.update_jobs(ui);
                }
            }
            "file-list" => {
                self.file_listing = result;
                self.validate_picker(ui);
                if self.page == "files" {
                    self.render_rows(ui);
                }
            }
            "file-history" => {
                self.file_history = result;
                if self.page == "files" {
                    self.render_rows(ui);
                }
            }
            "queue" => {
                self.queue = result;
                self.update_jobs(ui);
            }
            "updates" => self.set_updates(ui, result),
            "host-info" => {
                if self.host.is_null() {
                    self.host = json!({});
                }
                self.host["info"] = result;
            }
            "host-stats" => {
                if self.host.is_null() {
                    self.host = json!({});
                }
                self.host["stats"] = result;
            }
            "update-finished" => self.refresh(ui),
            "storage" => {
                let usage = &result["disk_usage"];
                let space = tr(ui, "storage_usage")
                    .replace(
                        "{used}",
                        &display_size(usage["used"].as_u64().unwrap_or_default()),
                    )
                    .replace(
                        "{available}",
                        &display_size(usage["free"].as_u64().unwrap_or_default()),
                    );
                self.dialog(ui, tr(ui, "g_code_storage"), &space, "info", "", "");
            }
            "history" => {
                let totals = &result["job_totals"];
                let filament = number(&totals["total_filament_used"]).unwrap_or_default() / 1000.;
                let details = tr(ui, "print_history_summary")
                    .replace(
                        "{prints}",
                        &totals["total_jobs"]
                            .as_u64()
                            .unwrap_or_default()
                            .to_string(),
                    )
                    .replace(
                        "{time}",
                        &duration(number(&totals["total_print_time"]).unwrap_or_default()),
                    )
                    .replace("{filament}", &format!("{filament:.1}"));
                self.dialog(ui, tr(ui, "print_history"), &details, "info", "", "");
            }
            "webcams" if self.page == "settings" && self.tab == "tools" => {
                let candidate = array(&result["webcams"]).iter().find_map(|camera| {
                    (camera["enabled"].as_bool() != Some(false))
                        .then(|| {
                            camera["snapshot_url"]
                                .as_str()
                                .filter(|url| !url.is_empty())
                        })
                        .flatten()
                });
                if let Some(url) = candidate {
                    self.webcam = moonraker::http_endpoint(self.endpoint.borrow().as_str(), &[])
                        .ok()
                        .and_then(|base| base.join(url).ok())
                        .map(|url| url.to_string());
                } else {
                    ui.set_message(tr(ui, "no_webcam_snapshot_url_is_configured").into());
                }
            }
            "gcode" | "start" | "emergency" | "pause" | "resume" | "cancel" | "restart"
            | "shutdown" | "reboot" => {}
            _ => {}
        }
        if matches!(tag, "queue" | "host-info" | "host-stats") {
            self.render_rows(ui);
        }
    }

    fn thumbnail(&self, tag: &str, path: &str, metadata: &Value) {
        if let Some(path) = print_files::thumbnail_path(path, array(&metadata["thumbnails"])) {
            let mut segments = vec!["server", "files", "gcodes"];
            segments.extend(path.split('/'));
            if let Ok(url) = moonraker::http_endpoint(self.endpoint.borrow().as_str(), &segments) {
                self.fetch_image(tag.to_owned(), url.as_str());
            }
        }
    }

    fn fetch_image(&self, tag: String, url: &str) {
        let client = self.http.clone();
        let tx = self.background.clone();
        let url = url.to_owned();
        let image_too_large = locale::text(
            "image_too_large",
            self.config["system"]["language"].as_str() == Some("de"),
        );
        self.runtime.spawn(async move {
            let result = async {
                const MAX_IMAGE: usize = 12 * 1024 * 1024;
                let mut response = client.get(url).send().await?.error_for_status()?;
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await? {
                    if bytes.len() + chunk.len() > MAX_IMAGE {
                        return Err(image_too_large.into());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(bytes)
            }
            .await
            .map_err(|error| error.to_string());
            let _ = tx.send(Background::Image(tag, result));
        });
    }

    fn background_event(&mut self, ui: &Moondash, event: Background) {
        match event {
            Background::Reply(tag, result) => self.reply(ui, &tag, result),
            Background::Sleeping(sleeping) => ui.set_sleeping(sleeping),
            Background::Image(tag, result) => {
                if tag == "webcam" {
                    self.webcam_image_pending = false;
                }
                match result.and_then(|bytes| {
                    Image::load_from_data(&bytes, None).map_err(|error| format!("{error:?}"))
                }) {
                    Ok(image) if tag == "webcam" && self.webcam.is_some() => {
                        ui.set_tool_image(image)
                    }
                    Ok(image) if tag.strip_prefix("preview:") == Some(self.filename.as_str()) => {
                        ui.set_preview(image)
                    }
                    Ok(image)
                        if tag.strip_prefix("file:") == Some(self.dialog.argument.as_str())
                            && (ui.get_dialog_mode() == "print"
                                || ui.get_dialog_mode().starts_with("file-")) =>
                    {
                        ui.set_dialog_image(image)
                    }
                    Ok(image) if tag.starts_with("card:") => {
                        self.file_previews
                            .insert(tag["card:".len()..].to_owned(), image);
                        if self.page == "files" {
                            self.render_rows(ui);
                        }
                    }
                    Err(error) if tag == "webcam" && self.webcam.is_some() => {
                        self.webcam = None;
                        ui.set_message(error.into());
                    }
                    _ => {}
                }
            }
        }
    }

    fn update_network_models(&self, ui: &Moondash) {
        let wifi = &self.network["wifi"];
        ui.set_network_wifi_available(wifi.is_object());
        ui.set_network_wifi_enabled(wifi["enabled"].as_bool().unwrap_or_default());
        ui.set_network_connected_ssid(
            wifi["connectedSsid"]
                .as_str()
                .or_else(|| self.network["status"]["ssid"].as_str())
                .unwrap_or_default()
                .into(),
        );
        ui.set_network_wifi_ip(wifi["connectedIp"].as_str().unwrap_or_default().into());
        ui.set_network_wifi_error(
            self.network["wifi-error"]
                .as_str()
                .unwrap_or_default()
                .into(),
        );

        let wifi_entries = |field| {
            array(&wifi[field])
                .iter()
                .filter_map(|network| {
                    let signal = network["signalPercent"].as_u64();
                    let icon = match signal {
                        Some(80..) => "wifi-strength-4",
                        Some(60..=79) => "wifi-strength-3",
                        Some(40..=59) => "wifi-strength-2",
                        Some(20..=39) => "wifi-strength-1",
                        _ => "wifi-strength-outline",
                    };
                    Some(WifiEntry {
                        ssid: network["ssid"].as_str()?.into(),
                        signal: signal
                            .map_or_else(
                                || tr(ui, "network_none").to_owned(),
                                |signal| format!("{signal}%"),
                            )
                            .into(),
                        icon: icons::glyph(icon),
                        saved: network["saved"].as_bool().unwrap_or_default(),
                        secured: network["secured"].as_bool().unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>()
        };
        set_model(
            &ui.get_network_scanned(),
            &wifi_entries("scannedNetworks"),
            |model| ui.set_network_scanned(model),
        );
        set_model(
            &ui.get_network_saved(),
            &wifi_entries("savedNetworks"),
            |model| ui.set_network_saved(model),
        );

        let wired: Vec<_> = array(&self.network["wired"]["interfaces"])
            .iter()
            .filter_map(|interface| {
                let name = interface["interfaceName"].as_str()?;
                let active = interface["connected"].as_bool().unwrap_or_default();
                Some(Row {
                    active,
                    ..row(
                        name,
                        interface["ip"].as_str().unwrap_or("--"),
                        if active { "wired-off" } else { "wired-on" },
                        name,
                        "",
                    )
                })
            })
            .collect();
        set_model(&ui.get_network_wired(), &wired, |model| {
            ui.set_network_wired(model)
        });

        let can: Vec<_> = array(&self.network["can"]["interfaces"])
            .iter()
            .filter_map(|interface| {
                let name = interface["interfaceName"].as_str()?;
                let mut detail = Vec::new();
                if let Some(bitrate) = interface["bitrate"].as_u64() {
                    detail.push(if bitrate < 1_000 {
                        format!("{bitrate} bit/s")
                    } else {
                        format!("{:.1} kbit/s", bitrate as f64 / 1_000.)
                    });
                }
                let rx = number(&interface["rxBytesPerSecond"]);
                let tx = number(&interface["txBytesPerSecond"]);
                if rx.is_some() || tx.is_some() {
                    let rate = |bytes: Option<f64>| {
                        bytes.map_or_else(
                            || "--".to_owned(),
                            |value| format!("{}/s", display_size(value as u64)),
                        )
                    };
                    detail.push(format!("RX {} / TX {}", rate(rx), rate(tx)));
                }
                Some(Row {
                    active: interface["connected"].as_bool().unwrap_or_default(),
                    ..info(
                        name,
                        if detail.is_empty() {
                            "--".to_owned()
                        } else {
                            detail.join(" · ")
                        },
                    )
                })
            })
            .collect();
        set_model(&ui.get_network_can(), &can, |model| {
            ui.set_network_can(model)
        });
    }

    fn network_reply(&mut self, ui: &Moondash, action: &str, reply: Result<Value, String>) {
        if action == "network" && ui.get_network_job() != "network" {
            return;
        }
        ui.set_network_job("".into());
        ui.set_network_job_target("".into());
        let result = match reply {
            Ok(result) => result,
            Err(error) => {
                if action == "wifi-scan" {
                    self.network["wifi"]["scannedNetworks"] = json!([]);
                    self.update_network_models(ui);
                }
                ui.set_network_error(error.into());
                return;
            }
        };
        if action == "wifi-scan" {
            self.network["wifi"]["scannedNetworks"] = result["scanned"].clone();
            self.network["wifi"]["savedNetworks"] = result["saved"].clone();
            self.update_network_models(ui);
            ui.set_network_scan_cached(result["cached"].as_bool().unwrap_or_default());
            return;
        }
        if self.network["ip"] != result["ip"] {
            let url = result["ip"].as_str().map(|ip| {
                if ip.contains(':') {
                    format!("http://[{ip}]")
                } else {
                    format!("http://{ip}")
                }
            });
            ui.set_web_qr(url.as_deref().map_or_else(Image::default, web_qr));
            ui.set_web_url(url.unwrap_or_default().into());
        }
        self.network = result;
        self.update_network_models(ui);

        match action {
            "wifi-connect" | "wifi-hidden-password" => {
                ui.set_network_dialog("".into());
                ui.set_network_password("".into());
                ui.set_network_ssid("".into());
            }
            _ => {}
        }
    }

    fn network_action(&self, ui: &Moondash, action: &str, argument: &str, password: &str) {
        let job = ui.get_network_job();
        if !(job.is_empty() || (job == "network" && action == "wifi-scan"))
            || (action == "network" && !ui.get_network_dialog().is_empty())
        {
            return;
        }
        if action != "network" {
            ui.set_network_error("".into());
        }
        if action == "wifi-scan" {
            ui.set_network_scan_cached(false);
            ui.set_network_dialog("scan".into());
        }
        ui.set_network_job(action.into());
        ui.set_network_job_target(argument.into());
        let (action, argument) = (action.to_owned(), argument.to_owned());
        let password = (!password.is_empty()).then(|| password.to_owned());
        let tx = self.background.clone();
        self.runtime.spawn(async move {
            let tag = format!("network:{action}");
            if action == "wifi-scan" {
                let result = network::scan_wifi_networks().await.map(|scan| json!(scan));
                let _ = tx.send(Background::Reply(tag, result));
                return;
            }
            let result = match action.as_str() {
                "network" => Ok(()),
                "wifi-on" | "wifi-off" => network::set_wifi_enabled(action == "wifi-on").await,
                "wired-on" | "wired-off" => network::set_wired_interface_enabled(argument, action == "wired-on").await,
                "wifi-connect" => network::connect_to_wifi(argument, password).await,
                "wifi-hidden-password" => network::connect_hidden_wifi(argument, password).await,
                "wifi-forget" => network::forget_saved_wifi(argument).await,
                _ => return,
            };
            if let Err(error) = result {
                let _ = tx.send(Background::Reply(tag, Err(error)));
                return;
            }
            let (ip, wifi, wired, can, status) = tokio::join!(network::get_primary_ip_address(), network::get_wifi_settings(),
                network::get_wired_settings(), network::get_canbus_settings(), network::get_network_status());
            let wifi_error = wifi.as_ref().err().cloned();
            let data = json!({"ip": ip.ok(), "wifi": wifi.ok(), "wifi-error": wifi_error, "wired": wired.ok(), "can": can.ok(), "status": status.ok()});
            let _ = tx.send(Background::Reply(tag, Ok(data)));
        });
    }
}

fn gcode_arg(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"_-#.".contains(&byte))
    {
        value.to_owned()
    } else {
        serde_json::to_string(value).unwrap_or_default()
    }
}

fn shortcut_active(status: &Map<String, Value>, shortcut: &Value) -> bool {
    let Some(config) = shortcut["active_config"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
    else {
        return false;
    };
    let kind = shortcut["active_type"].as_str().unwrap_or_default();
    let (name, channel) = if kind == "led" {
        config
            .trim()
            .rsplit_once(' ')
            .map_or((config, ""), |(name, channel)| (name.trim(), channel))
    } else {
        (config.trim(), "")
    };
    let typed = format!("{kind} {name}");
    let object = status
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name) || key.eq_ignore_ascii_case(&typed))
        .or_else(|| {
            status.iter().find(|(key, _)| {
                key.split_once(' ')
                    .is_some_and(|(_, suffix)| suffix.eq_ignore_ascii_case(name))
            })
        })
        .map(|(_, value)| value)
        .unwrap_or(&Value::Null);
    let numeric = |value: &Value| {
        number(value).or_else(|| {
            value
                .as_str()?
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
        })
    };
    if kind == "led" {
        let Some(index) = ["red", "green", "blue", "white"]
            .iter()
            .position(|key| key.eq_ignore_ascii_case(channel))
        else {
            return false;
        };
        return numeric(&object["color_data"][0][index])
            .is_some_and(|value| value >= number(&shortcut["active_threshold"]).unwrap_or(0.5));
    }
    let fields = if matches!(kind, "fan" | "fan_generic" | "temperature_fan") {
        ["speed", "value", "state", "status"]
    } else {
        ["value", "speed", "state", "status"]
    };
    let value = fields
        .iter()
        .find_map(|key| object.get(*key))
        .unwrap_or(object);
    if matches!(
        kind,
        "fan" | "fan_generic" | "temperature_fan" | "output_pin"
    ) {
        return numeric(value).is_some_and(|value| {
            value >= number(&shortcut["active_threshold"]).unwrap_or_default()
        });
    }
    value
        .as_bool()
        .or_else(|| numeric(value).map(|value| value > 0.))
        .unwrap_or_else(|| {
            value.as_str().is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "on" | "true" | "enabled"
                )
            })
        })
}

fn save_config(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;
    let path = if path.is_symlink() {
        path.canonicalize()?
    } else {
        path.to_owned()
    };
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    if let Ok(metadata) = fs::metadata(&path) {
        temp.as_file().set_permissions(metadata.permissions())?;
    }
    temp.write_all(text.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}

fn start_display_watcher(
    config: Arc<Mutex<Value>>,
    manual_sleep: Arc<AtomicBool>,
    tx: mpsc::Sender<Background>,
) {
    std::thread::spawn(move || {
        let mut devices = Vec::new();
        let mut scanned = Instant::now() - Duration::from_secs(10);
        let mut last_activity = Instant::now();
        let mut sleeping = false;

        loop {
            if scanned.elapsed() >= Duration::from_secs(10) {
                scanned = Instant::now();
                devices = input_idle::open_input_devices().unwrap_or_default();
            }
            let mut active = false;
            for device in &mut devices {
                if let Ok(events) = device.fetch_events() {
                    active |= events
                        .into_iter()
                        .any(|event| input_idle::is_real_input_event(event.event_type()));
                }
            }

            let (enabled, timeout, unlock, german) = config
                .lock()
                .map(|config| {
                    (
                        config["use_idle_timeout"].as_bool().unwrap_or_default(),
                        Duration::from_secs(config["idle_timeout"].as_u64().unwrap_or(900).max(1)),
                        Duration::from_millis(config["idle_unlock"].as_u64().unwrap_or(500)),
                        config["language"].as_str() == Some("de"),
                    )
                })
                .unwrap_or_default();
            if active {
                last_activity = Instant::now();
            }
            if sleeping && active {
                for _ in 0..3 {
                    if wayland_power::turn_on_displays().is_ok() {
                        sleeping = false;
                        std::thread::sleep(unlock);
                        last_activity = Instant::now();
                        let _ = tx.send(Background::Sleeping(false));
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            let manual = manual_sleep.swap(false, Ordering::Relaxed);
            if !sleeping && (manual || enabled && last_activity.elapsed() >= timeout) {
                if devices.is_empty() {
                    if manual {
                        let _ = tx.send(Background::Reply(
                            "display".into(),
                            Err(locale::text("no_display_wake_device", german).into()),
                        ));
                    }
                    last_activity = Instant::now();
                } else {
                    match wayland_power::turn_off_displays() {
                        Ok(()) => {
                            sleeping = true;
                            let _ = tx.send(Background::Sleeping(true));
                        }
                        Err(error) => {
                            last_activity = Instant::now();
                            let _ = tx.send(Background::Reply("display".into(), Err(error)));
                        }
                    }
                }
            }
            std::thread::sleep(if sleeping {
                Duration::from_millis(60)
            } else {
                Duration::from_millis(250)
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{format_speed_percent, klipper_message, warning_messages};
    use serde_json::json;

    #[test]
    fn extracts_klipper_runtime_and_moonraker_warnings() {
        let runtime_warning = "MCU configured for 20Mhz but running at 1Mhz!";
        let klipper = json!({
            "webhooks": {"state": "ready", "state_message": "Printer is ready"},
            "configfile": {"warnings": [
                {"type": "runtime_warning", "message": runtime_warning},
                {"type": "runtime_warning", "message": runtime_warning},
            ]},
        });
        let moonraker = json!({"warnings": [" Server warning ", "", null]});

        assert_eq!(klipper_message(&klipper["webhooks"]["state_message"]), None);
        assert_eq!(
            warning_messages(&klipper["configfile"]["warnings"]).collect::<Vec<_>>(),
            [runtime_warning]
        );
        assert_eq!(
            warning_messages(&moonraker["warnings"]).collect::<Vec<_>>(),
            ["Server warning"]
        );
    }

    #[test]
    fn speed_percent_uses_truncated_thousands_above_999() {
        for (percent, expected) in [
            (999, "999%"),
            (1_000, "1k"),
            (1_099, "1.09k"),
            (1_250, "1.25k"),
            (1_266, "1.26k"),
            (1_999, "1.99k"),
        ] {
            assert_eq!(format_speed_percent(percent), expected);
        }
    }
}
