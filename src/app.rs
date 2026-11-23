use crate::icons;
use crate::{
    Lane, Moondash, Palette, Row, config, input_idle,
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
    page: String,
    tab: String,
    folder: String,
    directory: Value,
    filter: String,
    file_sort: u8,
    file_details: HashMap<String, String>,
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
        page: "home".into(),
        tab: String::new(),
        folder: String::new(),
        directory: Value::Null,
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
        filename: String::new(),
        metadata: Value::Null,
        jog_step: 10.,
        config_checked: Instant::now(),
        refresh_at: Instant::now(),
        webcam: None,
        webcam_image_pending: false,
    }));

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
    ui.on_config_edited(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_config_dirty(true);
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
    ui.on_dialog_close(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_dialog_mode("".into());
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
                if force || !ui.get_config_dirty() {
                    ui.set_config_text(text.into());
                    ui.set_config_dirty(false);
                }
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
        self.endpoint.send_if_modified(|endpoint| {
            if *endpoint == host {
                return false;
            }
            *endpoint = host;
            true
        });

        let dark = self.config["styling"]["darkmode"].as_bool().unwrap_or(true);
        ui.set_dark(dark);
        let palette = ui.global::<Palette>();
        palette.set_dark(dark);
        palette.set_accent(config_color(&self.config["styling"]["primary"], 0x2196f3));
        ui.set_german(self.config["system"]["language"].as_str() == Some("de"));
        ui.set_debug(self.config["dev"]["debug"].as_bool().unwrap_or(false));
        if let Some(logo) = theme::custom_logo(&self.config_path) {
            ui.set_logo(logo);
        }
        palette.set_secondary(config_color(&self.config["styling"]["secondary"], 0x48a9a6));
        let profile = self.config["styling"]["display_profile"]
            .as_str()
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
            match (self.page.as_str(), self.tab.as_str()) {
                ("settings", "network") => self.network_action("network", "", ""),
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
            ("files", _) if self.online => self.call(
                ui,
                "server.files.get_directory",
                json!({
                    "path": gcode_path(&self.folder), "extended": true
                }),
                format!("directory:{}", self.folder),
            ),
            ("home", _) | ("tune", "queue") if self.online => {
                self.call(ui, "server.job_queue.status", json!({}), "queue")
            }
            ("settings", "network") => self.network_action("network", "", ""),
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
        if !self.online {
            ui.set_message("Moonraker is disconnected".into());
        } else if self.calls.try_send(Call::new(method, params, tag)).is_err() {
            ui.set_message("Moonraker request queue is busy".into());
        }
    }

    fn motion(&mut self, ui: &Moondash, script: impl Into<String>) {
        if !ui.get_ready() || ui.get_printing() || ui.get_paused() || self.motion_busy {
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
            ui.set_message("Moonraker request queue is busy".into());
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
            ui.set_message("Klipper is not ready".into());
        }
    }

    fn event(&mut self, ui: &Moondash, event: Event) {
        match event {
            Event::Connected => {
                self.online = true;
                ui.set_online(true);
                ui.set_connection("Moonraker connected".into());
                ui.set_message("".into());
                self.refresh(ui);
            }
            Event::Disconnected(reason) => {
                if self.online {
                    self.online = false;
                    self.status.clear();
                    self.directory = Value::Null;
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
                ui.set_online(false);
                ui.set_ready(false);
                ui.set_connection(format!("Moonraker • {reason}").into());
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
                    if value["complete"].as_bool() == Some(true) && self.tab == "updates" {
                        self.refresh(ui);
                    }
                }
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
        let webhooks = self.object("webhooks");
        let klippy = webhooks["state"].as_str().unwrap_or("connecting");
        ui.set_ready(self.online && klippy == "ready");
        if self.online {
            ui.set_connection(format!("Moonraker • Klipper {klippy}").into());
        }
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

    fn object(&self, key: &str) -> &Value {
        self.status.get(key).unwrap_or(&Value::Null)
    }

    fn heater_rows(&self) -> impl Iterator<Item = Row> + '_ {
        self.status
            .iter()
            .filter(|(name, _)| {
                *name == "heater_bed"
                    || name.starts_with("extruder") && !name.contains('_')
                    || name.starts_with("heater_generic ")
            })
            .filter_map(|(name, data)| {
                let current = number(&data["temperature"])?;
                let target = number(&data["target"]).unwrap_or_default();
                Some(row(
                    format!("{name}  {current:.0}°C"),
                    format!("Target {target:.0}°C"),
                    "heater",
                    name,
                    "Set",
                ))
            })
    }

    fn update_shortcuts(&self, ui: &Moondash) {
        let german = ui.get_german();
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
                "Set",
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
        let bed = if german { "Heizbett" } else { "Heater Bed" };
        let mut controls = vec![
            heater(tool, "Extruder", "printer-3d-nozzle-heat"),
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
        for (key, data) in &self.status {
            let Some(speed) = number(&data["speed"]) else {
                continue;
            };
            let (name, action) = if key == "fan" {
                (if german { "Bauteillüfter" } else { "Part Fan" }, "fan")
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
            let mut entry = row(name, format!("{:.0}%", speed * 100.), action, key, "Set");
            entry.icon = icons::glyph("fan");
            entry.tone = ink;
            controls.push(entry);
        }
        let factor = number(&self.object("gcode_move")["speed_factor"]);
        let mut speed = row(
            factor
                .map(|speed| format!("{:.0}%", speed * 100.))
                .unwrap_or_else(|| "--".into()),
            "",
            "speed",
            "",
            "Set",
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
                let mut button = row(name, "", "shortcut", name, "Run");
                button.active = shortcut_active(&self.status, item);
                button.icon = icons::glyph(item["icon"].as_str().unwrap_or_default());
                if button.icon.is_empty() {
                    button.icon = icons::glyph("flash");
                }
                Some(button)
            })
            .collect();

        let sensors: Vec<Row> = self
            .status
            .iter()
            .filter_map(|(key, data)| {
                let name = key
                    .strip_prefix("filament_switch_sensor ")
                    .or_else(|| key.strip_prefix("filament_motion_sensor "))?;
                if name.to_ascii_lowercase().starts_with("afc") {
                    return None;
                }
                let detected = data["filament_detected"].as_bool().unwrap_or_default();
                let label = match (detected, german) {
                    (true, true) => "Erkannt",
                    (true, false) => "Detected",
                    (false, true) => "Leer",
                    (false, false) => "Empty",
                };
                Some(Row {
                    tone: if detected { rgb(0x4caf50) } else { ink },
                    ..info(name, label)
                })
            })
            .collect();
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
                    } else if ui.get_german() {
                        "Leer"
                    } else {
                        "Empty"
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

fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
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
    let color = value
        .as_str()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('#');
    rgb(u32::from_str_radix(color, 16)
        .ok()
        .filter(|_| color.len() == 6)
        .unwrap_or(default))
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

fn directory_rows(directory: &Value, folder: &str, filter: &str, sort: u8) -> Vec<Row> {
    let filter = filter.to_lowercase();
    let mut rows = Vec::new();
    for (key, name_key, action, remove, button) in [
        ("dirs", "dirname", "folder", "remove-dir", "Open"),
        ("files", "filename", "file", "remove-file", "Details"),
    ] {
        let mut entries: Vec<_> = array(&directory[key])
            .iter()
            .filter_map(|item| {
                let name = item
                    .as_str()
                    .or_else(|| item[name_key].as_str())
                    .or_else(|| item["name"].as_str())?;
                (valid_path(name)
                    && !name.contains('/')
                    && !name.starts_with('.')
                    && name.to_lowercase().contains(&filter))
                .then_some((name, item))
            })
            .collect();
        entries.sort_by_cached_key(|(name, _)| name.to_lowercase());
        if sort == 3 {
            entries.reverse();
        }
        if sort < 2 {
            entries.sort_by(|(_, first), (_, second)| {
                let order = number(&first["modified"])
                    .unwrap_or_default()
                    .total_cmp(&number(&second["modified"]).unwrap_or_default());
                if sort == 0 { order.reverse() } else { order }
            });
        }
        for (name, item) in entries {
            let path = join_path(folder, name);
            let detail = item["size"].as_u64().map(display_size).unwrap_or_default();
            let mut entry = row(name, detail, action, &path, button);
            if item["permissions"]
                .as_str()
                .is_none_or(|permissions| permissions.contains('w'))
            {
                entry = alternate(entry, remove, "Delete");
            }
            rows.push(entry);
        }
    }
    rows
}

impl State {
    fn heater_limit(&self, heater: &str) -> f64 {
        let settings = &self.object("configfile")["settings"][heater];
        number(&settings["max_temp"]).unwrap_or_else(|| match heater {
            "heater_bed" => 120.,
            name if name.starts_with("extruder") => 300.,
            _ => 150.,
        })
    }

    fn render_rows(&self, ui: &Moondash) {
        let mut rows = Vec::new();
        match (self.page.as_str(), self.tab.as_str()) {
            ("files", _) => {
                ui.set_folder(self.folder.clone().into());
                rows = directory_rows(&self.directory, &self.folder, &self.filter, self.file_sort);
                for row in &mut rows {
                    if let Some(detail) = self.file_details.get(row.argument.as_str()) {
                        row.detail = detail.clone().into();
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
                        rows.push(info(name, "Excluded"));
                    } else {
                        rows.push(row(name, "", "exclude", name, "Skip"));
                    }
                }
                if rows.is_empty() {
                    rows.push(info("No printable objects", ""));
                }
            }
            ("tune", "queue") => {
                let state = self.queue["queue_state"].as_str().unwrap_or("—");
                rows.push(if state == "paused" {
                    row("Job queue", state, "queue-start", "", "Start queue")
                } else {
                    row("Job queue", state, "queue-pause", "", "Pause queue")
                });
                for job in array(&self.queue["queued_jobs"]) {
                    if let (Some(id), Some(filename)) =
                        (job["job_id"].as_str(), job["filename"].as_str())
                    {
                        rows.push(alternate(
                            row(filename, "", "file", filename, "Details"),
                            &format!("queue-remove:{id}"),
                            "Remove",
                        ));
                    }
                }
            }
            ("settings", "config") => {
                let system = &self.config["system"];
                let styling = &self.config["styling"];
                let text = |english, german| if ui.get_german() { german } else { english };
                let mut dark = row(
                    text("Dark mode", "Dunkelmodus"),
                    "",
                    "config-toggle",
                    "styling.darkmode",
                    text("On", "An"),
                );
                dark.active = styling["darkmode"].as_bool().unwrap_or(true);
                if !dark.active {
                    dark.button = text("Off", "Aus").into();
                }
                rows.push(dark);
                rows.push(alternate(
                    row(
                        text("Colors", "Farben"),
                        "",
                        "config-edit",
                        "styling.primary",
                        text("Primary", "Primär"),
                    ),
                    "config-secondary",
                    text("Secondary", "Sekundär"),
                ));
                let mut advanced = row(
                    text("Advanced state", "Erweiterte Anzeige"),
                    "",
                    "config-toggle",
                    "styling.advanced_states",
                    text("Off", "Aus"),
                );
                advanced.active = styling["advanced_states"].as_bool().unwrap_or(false);
                if advanced.active {
                    advanced.button = text("On", "An").into();
                }
                rows.push(advanced);
                rows.push(row(
                    text("Language", "Sprache"),
                    "",
                    "config-language",
                    "",
                    if ui.get_german() {
                        "Deutsch"
                    } else {
                        "English"
                    },
                ));
                rows.push(row(
                    text("Display profile", "Anzeigeprofil"),
                    "",
                    "config-profile",
                    "",
                    styling["display_profile"].as_str().unwrap_or("Auto"),
                ));
                let mut idle = row(
                    text("Display idle timeout", "Bildschirm-Ruhezeit"),
                    "",
                    "config-toggle",
                    "system.use_idle_timeout",
                    text("On", "An"),
                );
                idle.active = system["use_idle_timeout"].as_bool().unwrap_or(true);
                if !idle.active {
                    idle.button = text("Off", "Aus").into();
                }
                rows.push(idle);
                if system["use_idle_timeout"].as_bool().unwrap_or(true) {
                    rows.push(row(
                        text("Idle timeout", "Ruhezeit"),
                        format!("{} s", system["idle_timeout"]),
                        "config-edit",
                        "system.idle_timeout",
                        text("Edit", "Ändern"),
                    ));
                }
                rows.push(row(
                    "Moonraker",
                    self.endpoint.borrow().as_str(),
                    "config-edit",
                    "websocket.ip",
                    text("Edit", "Ändern"),
                ));
                rows.push(row(
                    text("Configuration file", "Konfigurationsdatei"),
                    self.config_path.display().to_string(),
                    "config-raw",
                    "",
                    text("Edit file", "Datei öffnen"),
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
                        if ui.get_german() {
                            "Bearbeiten"
                        } else {
                            "Edit"
                        },
                    );
                    entry.icon = icons::glyph(shortcut["icon"].as_str().unwrap_or_default());
                    rows.push(entry);
                }
                if rows.len() < 3 {
                    rows.push(row(
                        if ui.get_german() {
                            "Kurzbefehl hinzufügen"
                        } else {
                            "Add shortcut"
                        },
                        "",
                        "add-shortcut",
                        "",
                        "+",
                    ));
                }
            }
            ("settings", "network") => {
                let mut right = Vec::new();
                let ip = self.network["ip"].as_str().unwrap_or("—");
                rows.push(row("IP address", ip, "refresh", "", "Refresh"));
                if self.network["status"]["wifiConnected"].as_bool() == Some(true)
                    && let Some(quality) = self.network["status"]["quality"].as_str()
                {
                    rows.push(info("Wi-Fi signal", quality));
                }
                let wifi = &self.network["wifi"];
                if let Some(enabled) = wifi["enabled"].as_bool() {
                    rows.push(alternate(
                        row(
                            "Wi-Fi",
                            wifi["connectedSsid"].as_str().unwrap_or(""),
                            if enabled { "wifi-off" } else { "wifi-on" },
                            "",
                            if enabled { "Disable" } else { "Enable" },
                        ),
                        "wifi-scan",
                        "Scan",
                    ));
                    if enabled {
                        rows.push(row("Hidden network", "", "wifi-hidden", "", "Connect"));
                    }
                }
                if let Some(error) = self.network["wifi-error"].as_str() {
                    rows.push(info("Wi-Fi", error));
                }
                for item in array(&wifi["scannedNetworks"]) {
                    let Some(ssid) = item["ssid"].as_str() else {
                        continue;
                    };
                    let signal = item["signalPercent"]
                        .as_u64()
                        .map(|signal| format!("{signal}%"))
                        .unwrap_or_default();
                    rows.push(row(ssid, signal, "wifi-connect", ssid, "Connect"));
                }
                for item in array(&wifi["savedNetworks"]) {
                    let Some(ssid) = item["ssid"].as_str() else {
                        continue;
                    };
                    rows.push(alternate(
                        row(ssid, "Saved Wi-Fi network", "wifi-connect", ssid, "Connect"),
                        "wifi-forget",
                        "Forget",
                    ));
                }
                for item in array(&self.network["wired"]["interfaces"]) {
                    let Some(name) = item["interfaceName"].as_str() else {
                        continue;
                    };
                    let connected = item["connected"].as_bool() == Some(true);
                    rows.push(row(
                        name,
                        item["ip"].as_str().unwrap_or("Ethernet"),
                        if connected { "wired-off" } else { "wired-on" },
                        name,
                        if connected { "Disable" } else { "Enable" },
                    ));
                }
                for item in array(&self.network["can"]["interfaces"]) {
                    let Some(name) = item["interfaceName"].as_str() else {
                        continue;
                    };
                    right.push(info(
                        name,
                        format!(
                            "CAN • {} bit/s • RX {} • TX {}",
                            item["bitrate"], item["rxPackets"], item["txPackets"]
                        ),
                    ));
                }
                set_model(&ui.get_trailing_rows(), &right, |model| {
                    ui.set_trailing_rows(model)
                });
            }
            ("settings", "tools") => match self.tool_view.as_str() {
                "mesh" => {
                    rows.push(row(
                        "Bed mesh",
                        self.object("bed_mesh")["profile_name"]
                            .as_str()
                            .unwrap_or_default(),
                        "mesh-info",
                        "",
                        "Inspect",
                    ));
                    rows.push(alternate(
                        row(
                            "Bed mesh calibration",
                            "",
                            "mesh-calibrate",
                            "",
                            "Calibrate",
                        ),
                        "mesh-clear",
                        "Clear",
                    ));
                    if let Some(profiles) = self.object("bed_mesh")["profiles"].as_object() {
                        for name in profiles.keys() {
                            rows.push(alternate(
                                row(name.as_str(), "Bed mesh profile", "mesh-load", name, "Load"),
                                "mesh-remove",
                                "Remove",
                            ));
                        }
                    }
                    rows.push(row(
                        "Save printer configuration",
                        "Klipper restarts after saving",
                        "save-printer",
                        "",
                        "Save",
                    ));
                }
                "pid" => {
                    for heater in self.heater_rows() {
                        rows.push(row(
                            heater.argument.clone(),
                            "PID calibration",
                            "pid",
                            heater.argument.as_str(),
                            "Tune",
                        ));
                    }
                    rows.push(row(
                        "Save printer configuration",
                        "Klipper restarts after saving",
                        "save-printer",
                        "",
                        "Save",
                    ));
                }
                "camera" => {}
                _ => {
                    for (title, action, icon) in [
                        (
                            if ui.get_german() {
                                "Druckbettvermessung"
                            } else {
                                "Bed Mesh"
                            },
                            "tool-mesh",
                            "grid",
                        ),
                        (
                            if ui.get_german() {
                                "PID-Kalibrierung"
                            } else {
                                "PID Tuning"
                            },
                            "tool-pid",
                            "tune-variant",
                        ),
                        ("Webcam", "tool-camera", "webcam"),
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
                    "Refresh",
                ));
                let system = &self.host["info"]["system_info"];
                let stats = &self.host["stats"];
                rows.push(info(
                    "Host",
                    system["distribution"]["pretty_name"]
                        .as_str()
                        .unwrap_or("—"),
                ));
                rows.push(info(
                    "CPU",
                    system["cpu_info"]["cpu_desc"].as_str().unwrap_or("—"),
                ));
                if let Some(cpu) = number(&stats["system_cpu_usage"]["cpu"]) {
                    rows.push(info("CPU usage", format!("{cpu:.1}%")));
                }
                if let Some(memory) = stats["system_memory"].as_object() {
                    rows.push(info(
                        "Memory",
                        format!(
                            "{} / {}",
                            display_size(memory["used"].as_u64().unwrap_or_default() * 1024),
                            display_size(memory["total"].as_u64().unwrap_or_default() * 1024)
                        ),
                    ));
                }
                rows.push(row("G-code storage", "", "storage", "", "Inspect"));
                rows.push(row("Print history", "", "history", "", "Open"));
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
                    row("Software updates", "", "update-refresh", "", "Check"),
                    "update-all",
                    "Update all",
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
                            row(name.as_str(), detail, "update", name, "Update")
                        } else {
                            info(name.as_str(), detail)
                        });
                    }
                }
                rows.push(row(
                    "Moondash service",
                    "After a Moondash package update",
                    "restart-service",
                    "",
                    "Restart",
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
                "refresh" if self.tab == "network" => "lan",
                "wifi-on" | "wifi-off" | "wifi-connect" | "wifi-hidden" => "wifi",
                "wired-on" | "wired-off" => "ethernet",
                "webcam" => "webcam",
                "mesh-info" | "mesh-calibrate" => "grid",
                "pid" => "tune-variant",
                "history" => "clock-outline",
                "restart-klipper" | "restart-service" => "restart",
                "reboot" | "sleep" => "power",
                "config-raw" => "code-json",
                "config-edit" => "pencil",
                "add-shortcut" => "lightning-bolt",
                _ => "",
            });
        }
        set_model(&ui.get_rows(), &rows, |model| ui.set_rows(model));
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
        ui.set_dialog_queue(false);
        ui.set_dialog_move(false);
        ui.set_dialog_folder(false);
        ui.set_dialog_icon("".into());
        ui.set_dialog_unit("".into());
        ui.set_dialog_current("".into());
        ui.set_dialog_limit("".into());
        ui.set_dialog_secret(false);
        ui.set_dialog_mode(mode.into());
    }

    fn action(&mut self, ui: &Moondash, action: &str, argument: &str) {
        match action {
            "dismiss" => ui.set_message("".into()),
            "refresh" => self.refresh(ui),
            "tool-mesh" | "tool-pid" | "tool-camera" => {
                let (view, title) = match action {
                    "tool-mesh" => ("mesh", "Bed Mesh"),
                    "tool-pid" => ("pid", "PID Tuning"),
                    _ => ("camera", "Webcam"),
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
            "power" => self.dialog(ui, "Power options", "", "power", "", ""),
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
                    ("Material", "afc-material", "printer-3d-nozzle"),
                    ("Color", "afc-color", "palette"),
                    ("Weight", "afc-weight", "scale"),
                    ("Tool mapping", "afc-map", "source-branch"),
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
            "file-filter" => {
                self.dialog(
                    ui,
                    if ui.get_german() {
                        "Dateien filtern"
                    } else {
                        "Filter files"
                    },
                    "",
                    "text",
                    action,
                    "",
                );
                ui.set_dialog_value(self.filter.clone().into());
            }
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
                        entry.detail.as_str(),
                        "file-context",
                        entry.alternate.as_str(),
                        argument,
                    );
                    ui.set_dialog_folder(entry.action == "folder");
                    ui.set_dialog_image(entry.preview);
                    ui.set_dialog_move(true);
                }
            }
            "delete-selected" => {
                let (action, path) = (self.dialog.action.clone(), self.dialog.argument.clone());
                self.action(ui, &action, &path);
            }
            "config-raw" => ui.set_config_view("raw".into()),
            "config-form" => ui.set_config_view("".into()),
            "config-language" => self.change_setting(
                ui,
                "system.language",
                if ui.get_german() { "en" } else { "de" },
            ),
            "config-toggle" => {
                if let Some((section, key)) = argument.split_once('.') {
                    let value = !self.config[section][key].as_bool().unwrap_or_default();
                    self.change_setting(ui, argument, if value { "true" } else { "false" });
                }
            }
            "config-profile" => {
                let next = match self.config["styling"]["display_profile"].as_str() {
                    Some("small") => "normal",
                    Some("normal") => "highres",
                    Some("highres") => "auto",
                    _ => "small",
                };
                self.change_setting(ui, "styling.display_profile", next);
            }
            "config-secondary" => self.action(ui, "config-edit", "styling.secondary"),
            "config-edit" => {
                let Some((section, key)) = argument.split_once('.') else {
                    return;
                };
                let value = &self.config[section][key];
                let initial = value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned);
                let (title, detail) = match argument {
                    "styling.primary" => ("Primary color", "Hex color, e.g. #2196f3"),
                    "styling.secondary" => ("Secondary color", "Hex color, e.g. #48a9a6"),
                    "system.idle_timeout" => ("Idle timeout", "Seconds"),
                    _ => ("Moonraker", "Host and port or URL"),
                };
                self.dialog(ui, title, detail, "text", action, argument);
                ui.set_dialog_value(initial.into());
            }
            "edit-shortcut" => {
                self.dialog(
                    ui,
                    argument,
                    "Macro to run. Edit the configuration file to set advanced options.",
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
            "add-shortcut" => self.dialog(ui, "Add shortcut", "Shortcut name", "text", action, ""),
            "emergency" => self.call(ui, "printer.emergency_stop", json!({}), "emergency"),
            "pause" => self.call(ui, "printer.print.pause", json!({}), "pause"),
            "resume" => self.call(ui, "printer.print.resume", json!({}), "resume"),
            "cancel" => self.dialog(ui, "Cancel print?", "", "confirm", action, argument),
            "folder" | "up" => {
                self.folder = if action == "up" {
                    parent_dir(&self.folder).to_owned()
                } else {
                    argument.to_owned()
                };
                self.directory = Value::Null;
                ui.set_file_page(0);
                self.refresh(ui);
            }
            "file" => {
                self.dialog(
                    ui,
                    argument.rsplit('/').next().unwrap_or(argument),
                    "Loading file details…",
                    "confirm",
                    action,
                    argument,
                );
                ui.set_dialog_queue(true);
                ui.set_dialog_move(true);
                if let Some(image) = self.file_previews.get(argument) {
                    ui.set_dialog_image(image.clone());
                }
                self.metadata = Value::Null;
                self.call(
                    ui,
                    "server.files.metadata",
                    json!({"filename": argument}),
                    format!("file:{argument}"),
                );
            }
            "new-folder" => self.dialog(
                ui,
                "New folder",
                "Name within the current folder",
                "text",
                action,
                argument,
            ),
            "rename" => {
                self.dialog(
                    ui,
                    "Move or rename",
                    "New path relative to gcodes",
                    "text",
                    action,
                    argument,
                );
                ui.set_dialog_value(argument.into());
            }
            "move-selected" => {
                let path = self.dialog.argument.clone();
                self.action(ui, "rename", &path);
            }
            "remove-dir" | "remove-file" => {
                self.dialog(ui, "Delete?", argument, "confirm", action, argument)
            }
            action if action.starts_with("queue-remove:") => {
                self.dialog(
                    ui,
                    "Remove queued job?",
                    argument,
                    "confirm",
                    "queue-remove",
                    &action["queue-remove:".len()..],
                );
            }
            "queue-start" => self.dialog(ui, "Start job queue?", "", "confirm", action, argument),
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
            "disable-motors" => self.dialog(ui, "Disable motors?", "", "confirm", action, argument),
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
            "step" | "extrude" | "retract" | "heater" | "fan" | "temperature-fan" | "speed"
            | "flow" | "pid" => {
                let (title, detail, initial) = match action {
                    "step" => ("Jog distance", "Millimetres", self.jog_step),
                    "extrude" => ("Extrude", "Millimetres", 5.),
                    "retract" => ("Retract", "Millimetres", 5.),
                    "heater" | "temperature-fan" => (
                        argument,
                        "Target temperature in °C",
                        number(&self.object(argument)["target"]).unwrap_or_default(),
                    ),
                    "fan" => (
                        argument,
                        "Fan speed in percent",
                        number(&self.object(argument)["speed"]).unwrap_or_default() * 100.,
                    ),
                    "speed" => (
                        "Print speed",
                        "Percent",
                        number(&self.object("gcode_move")["speed_factor"]).unwrap_or(1.) * 100.,
                    ),
                    "flow" => (
                        "Extrusion factor",
                        "Percent",
                        number(&self.object("gcode_move")["extrude_factor"]).unwrap_or(1.) * 100.,
                    ),
                    _ => (
                        "PID calibration",
                        "Target temperature in °C. Klipper will heat the selected heater.",
                        if argument == "heater_bed" { 60. } else { 200. },
                    ),
                };
                self.dialog(ui, title, detail, "number", action, argument);
                let (label, icon, unit, limit, current) = match action {
                    "heater" | "temperature-fan" | "pid" => {
                        let is_bed = argument == "heater_bed";
                        let limit = self.heater_limit(argument);
                        let current = number(&self.object(argument)["temperature"])
                            .map(|temp| format!("{temp:.0}°C"))
                            .unwrap_or_else(|| "--°C".into());
                        (
                            if action == "pid" {
                                title
                            } else if is_bed {
                                if ui.get_german() {
                                    "Heizbett"
                                } else {
                                    "Heater Bed"
                                }
                            } else if argument.starts_with("extruder") {
                                "Extruder"
                            } else {
                                title
                            },
                            if is_bed {
                                "radiator"
                            } else {
                                "printer-3d-nozzle-heat"
                            },
                            "°C",
                            format!("{limit:.0}°C"),
                            current,
                        )
                    }
                    "speed" | "flow" | "fan" => (
                        title,
                        if action == "fan" {
                            "fan"
                        } else {
                            "speedometer-medium"
                        },
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
            "macro" => self.dialog(ui, "Run macro?", argument, "confirm", action, argument),
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
                    "afc-load" => "Load lane?",
                    "afc-unload" => "Unload lane?",
                    _ => "Skip object?",
                },
                argument,
                "confirm",
                action,
                argument,
            ),
            "afc-material" | "afc-color" | "afc-map" | "afc-runout" | "afc-weight" => self.dialog(
                ui,
                match action {
                    "afc-material" => "Material",
                    "afc-color" => "Color",
                    "afc-map" => "Tool mapping",
                    "afc-weight" => "Weight (g)",
                    _ => "Runout destination",
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
                    "Bed mesh",
                    if matrix.is_empty() {
                        "No active mesh"
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
                    "mesh-calibrate" => "Calibrate bed mesh?",
                    "mesh-clear" => "Clear bed mesh?",
                    "mesh-remove" => "Remove profile?",
                    "save-printer" => "Save configuration and restart Klipper?",
                    "restart-klipper" => "Restart Klipper?",
                    "restart-firmware" => "Restart firmware?",
                    "reboot" => "Reboot host?",
                    "shutdown" => "Shut down host?",
                    _ => "Restart Moondash service?",
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
                ui.set_message("Finish the print before installing updates.".into());
            }
            "update-all" | "update" => self.dialog(
                ui,
                "Install updates?",
                argument,
                "confirm",
                action,
                argument,
            ),
            "wifi-on" | "wifi-off" | "wifi-scan" => self.network_action(action, argument, ""),
            "wifi-connect" => {
                let network = &self.network["wifi"];
                let saved = array(&network["savedNetworks"])
                    .iter()
                    .any(|item| item["ssid"].as_str() == Some(argument));
                let secured = array(&network["scannedNetworks"])
                    .iter()
                    .find(|item| item["ssid"].as_str() == Some(argument))
                    .is_some_and(|item| item["secured"].as_bool() == Some(true));
                if secured && !saved {
                    self.dialog(ui, "Wi-Fi password", argument, "text", action, argument);
                    ui.set_dialog_secret(true);
                } else {
                    self.network_action(action, argument, "");
                }
            }
            "wifi-hidden" => self.dialog(
                ui,
                "Hidden Wi-Fi network",
                "Network name (SSID)",
                "text",
                action,
                "",
            ),
            "wifi-forget" | "wired-on" | "wired-off" => self.dialog(
                ui,
                "Change network settings?",
                argument,
                "confirm",
                action,
                argument,
            ),
            "reload-config" => self.load_config(ui, true),
            "save-config" => self.store_config(ui, argument),
            "sleep" => {
                if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                    self.manual_sleep.store(true, Ordering::Relaxed);
                } else {
                    ui.set_message("Display power control requires Wayland".into());
                }
            }
            _ => {}
        }
    }

    fn submit(&mut self, ui: &Moondash, input: &str, queue: bool) {
        let action = self.dialog.action.clone();
        let argument = self.dialog.argument.clone();
        let value = input.trim();

        if ui.get_dialog_mode() == "number" {
            let limit = match action.as_str() {
                "step" | "extrude" | "retract" => 100.,
                "fan" => 100.,
                "speed" | "flow" => 500.,
                "heater" | "temperature-fan" | "pid" => self.heater_limit(&argument),
                _ => return,
            };
            let minimum = match action.as_str() {
                "step" | "extrude" | "retract" | "speed" | "flow" => 0.01,
                "pid" => 1.,
                _ => 0.,
            };
            let Ok(number) = value.parse::<f64>() else {
                ui.set_dialog_detail(format!("Enter a number from {minimum} to {limit}").into());
                return;
            };
            if !(minimum..=limit).contains(&number) {
                ui.set_dialog_detail(format!("Enter a number from {minimum} to {limit}").into());
                return;
            }
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
            "config-edit" => {
                let valid = match argument.as_str() {
                    "system.idle_timeout" => value.parse::<u64>().is_ok_and(|seconds| seconds > 0),
                    "styling.primary" | "styling.secondary" => {
                        let hex = value.trim_start_matches('#');
                        value.is_empty()
                            || hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                    }
                    "websocket.ip" => moonraker::endpoint(value).is_ok(),
                    _ => false,
                };
                if !valid || value.contains(['\n', '\r']) {
                    ui.set_dialog_detail("Enter a valid value".into());
                    return;
                }
                self.change_setting(ui, &argument, value);
            }
            "add-shortcut" => {
                if value.is_empty()
                    || value.contains(['[', ']', '\n', '\r'])
                    || self.config.get(format!("shortcutbutton {value}")).is_some()
                {
                    ui.set_dialog_detail("Enter a unique shortcut name".into());
                    return;
                }
                self.dialog(ui, value, "Macro to run", "text", "new-shortcut", value);
                return;
            }
            "edit-shortcut" | "new-shortcut" => {
                if value.is_empty() || value.contains(['\n', '\r']) {
                    ui.set_dialog_detail("Enter a single-line G-code command.".into());
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
            "file" if queue => self.call(
                ui,
                "server.job_queue.post_job",
                json!({"filenames": [argument]}),
                "queue",
            ),
            "file" if ui.get_ready() && !ui.get_printing() && !ui.get_paused() => {
                self.call(
                    ui,
                    "printer.print.start",
                    json!({"filename": argument}),
                    "start",
                );
            }
            "file" => {
                ui.set_dialog_detail(
                    "Printer is busy or unavailable. Add this file to the queue instead.".into(),
                );
                return;
            }
            "cancel" => self.call(ui, "printer.print.cancel", json!({}), "cancel"),
            "queue-start" => self.call(ui, "server.job_queue.start", json!({}), "queue"),
            "queue-remove" => self.call(
                ui,
                "server.job_queue.delete_job",
                json!({"job_ids": [argument]}),
                "queue",
            ),
            "new-folder" if valid_path(value) && !value.contains('/') => {
                self.call(
                    ui,
                    "server.files.post_directory",
                    json!({"path": gcode_path(&join_path(&self.folder, value))}),
                    "files-changed",
                );
            }
            "rename" if valid_path(value) && value != argument => {
                self.call(
                    ui,
                    "server.files.move",
                    json!({"source": gcode_path(&argument), "dest": gcode_path(value)}),
                    "files-changed",
                );
            }
            "new-folder" | "rename" => {
                ui.set_dialog_detail("Enter a valid name or relative path.".into());
                return;
            }
            "remove-dir" => self.call(
                ui,
                "server.files.delete_directory",
                json!({"path": gcode_path(&argument)}),
                "files-changed",
            ),
            "remove-file" => self.call(
                ui,
                "server.files.delete_file",
                json!({"path": gcode_path(&argument)}),
                "files-changed",
            ),
            "disable-motors" => self.motion(ui, "M84"),
            "macro" => self.script(ui, argument),
            "exclude" => self.script(ui, format!("EXCLUDE_OBJECT NAME={}", gcode_arg(&argument))),
            "afc-load" => self.script(ui, format!("TOOL_LOAD LANE={}", gcode_arg(&argument))),
            "afc-unload" => self.script(ui, format!("LANE_UNLOAD LANE={}", gcode_arg(&argument))),
            "afc-material" | "afc-color" | "afc-map" | "afc-runout" | "afc-weight" => {
                if value.is_empty() || value.contains(['\n', '\r']) {
                    ui.set_dialog_detail("Enter a value.".into());
                    return;
                }
                let parameter = match action.as_str() {
                    "afc-material" => "MATERIAL",
                    "afc-color" => "COLOR",
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
            "wifi-hidden" => {
                if value.is_empty() {
                    return;
                }
                self.dialog(
                    ui,
                    "Hidden network password",
                    "Leave blank for an open network.",
                    "text",
                    "wifi-hidden-password",
                    value,
                );
                ui.set_dialog_secret(true);
                return;
            }
            "wifi-hidden-password" | "wifi-connect" | "wifi-forget" | "wired-on" | "wired-off" => {
                self.network_action(&action, &argument, input);
            }
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
            "heater" => format!(
                "SET_HEATER_TEMPERATURE HEATER={} TARGET={value}",
                gcode_arg(name)
            ),
            "temperature-fan" => format!(
                "SET_TEMPERATURE_FAN_TARGET TEMPERATURE_FAN={} TARGET={value}",
                gcode_arg(name)
            ),
            "fan" if argument == "fan" => format!("M106 S{}", (value * 2.55).round()),
            "fan" => format!(
                "SET_FAN_SPEED FAN={} SPEED={}",
                gcode_arg(name),
                value / 100.
            ),
            "speed" => format!("M220 S{value}"),
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

    fn store_config(&mut self, ui: &Moondash, text: &str) {
        let result = config::parse(text)
            .and_then(|_| save_config(&self.config_path, text).map_err(|error| error.to_string()));
        match result {
            Ok(()) => self.load_config(ui, true),
            Err(error) => ui.set_message(error.into()),
        }
    }

    fn change_setting(&mut self, ui: &Moondash, path: &str, value: &str) {
        if let Some((section, key)) = path.split_once('.') {
            let existing = fs::read_to_string(&self.config_path)
                .ok()
                .map(slint::SharedString::from)
                .unwrap_or_else(|| ui.get_config_text());
            let text = config::with_value(existing.as_str(), section, key, value);
            self.store_config(ui, &text);
        }
    }

    fn reply(&mut self, ui: &Moondash, tag: &str, reply: Result<Value, String>) {
        if tag == "motion" {
            self.motion_busy = false;
            ui.set_motion_busy(false);
        }
        let result = match reply {
            Ok(result) => result,
            Err(error) => {
                if let Some(path) = tag.strip_prefix("file:") {
                    if self.dialog.argument == path && !ui.get_dialog_mode().is_empty() {
                        ui.set_dialog_detail(error.into());
                        ui.set_dialog_queue(true);
                        ui.set_dialog_move(true);
                        ui.set_dialog_mode("confirm".into());
                    }
                } else if !tag.starts_with("preview:")
                    && !tag.starts_with("card:")
                    && !(tag == "queue" && self.page == "home")
                {
                    ui.set_message(error.into());
                }
                return;
            }
        };
        if let Some(folder) = tag.strip_prefix("directory:") {
            if self.folder == folder {
                self.directory = result;
                if self.page == "files" {
                    self.render_rows(ui);
                }
            }
            return;
        }
        if let Some(path) = tag.strip_prefix("card:") {
            let detail = number(&result["estimated_time"])
                .map(duration)
                .unwrap_or_default();
            self.file_details.insert(path.to_owned(), detail);
            self.thumbnail(tag, path, &result);
            if self.page == "files" {
                self.render_rows(ui);
            }
            return;
        }
        if let Some(path) = tag.strip_prefix("preview:") {
            if self.filename == path {
                self.thumbnail(tag, path, &result);
            }
            return;
        }
        if let Some(path) = tag.strip_prefix("file:") {
            if self.dialog.action == "file"
                && self.dialog.argument == path
                && !ui.get_dialog_mode().is_empty()
            {
                self.metadata = result;
                let mut detail = Vec::new();
                if let Some(time) = number(&self.metadata["estimated_time"]) {
                    detail.push(duration(time));
                }
                if let Some(weight) = number(&self.metadata["filament_weight_total"]) {
                    detail.push(format!("{weight:.1} g"));
                }
                if let Some(size) = self.metadata["size"].as_u64() {
                    detail.push(display_size(size));
                }
                ui.set_dialog_detail(detail.join(" · ").into());
                ui.set_dialog_mode("confirm".into());
                ui.set_dialog_queue(true);
                ui.set_dialog_move(true);
                self.thumbnail(tag, path, &self.metadata);
            }
            return;
        }

        match tag {
            "queue" => {
                self.queue = result;
                self.update_jobs(ui);
            }
            "network" => {
                if self.network["ip"] != result["ip"]
                    && let Some(ip) = result["ip"].as_str()
                {
                    let host = if ip.contains(':') {
                        format!("[{ip}]")
                    } else {
                        ip.to_string()
                    };
                    let url = format!("http://{host}");
                    ui.set_web_qr(web_qr(&url));
                    ui.set_web_url(url.into());
                } else if result["ip"].is_null() {
                    ui.set_web_qr(Image::default());
                    ui.set_web_url("".into());
                }
                self.network = result;
            }
            "updates" => self.updates = result,
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
            "files-changed" | "update-finished" => self.refresh(ui),
            "storage" => {
                let usage = &result["disk_usage"];
                let space = format!(
                    "{} used • {} available",
                    display_size(usage["used"].as_u64().unwrap_or_default()),
                    display_size(usage["free"].as_u64().unwrap_or_default())
                );
                self.dialog(ui, "G-code storage", &space, "info", "", "");
            }
            "history" => {
                let totals = &result["job_totals"];
                let details = format!(
                    "Prints · {}\nPrint time · {}\nFilament · {:.1} m",
                    totals["total_jobs"].as_u64().unwrap_or_default(),
                    duration(number(&totals["total_print_time"]).unwrap_or_default()),
                    number(&totals["total_filament_used"]).unwrap_or_default() / 1000.
                );
                self.dialog(ui, "Print history", &details, "info", "", "");
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
                    ui.set_message("No webcam snapshot URL is configured".into());
                }
            }
            "gcode" | "start" | "emergency" | "pause" | "resume" | "cancel" | "restart"
            | "shutdown" | "reboot" => {}
            _ => {}
        }
        if matches!(
            tag,
            "queue" | "network" | "updates" | "host-info" | "host-stats"
        ) {
            self.render_rows(ui);
        }
    }

    fn thumbnail(&self, tag: &str, path: &str, metadata: &Value) {
        let thumbnails = array(&metadata["thumbnails"]);
        let thumb = if tag.starts_with("card:") {
            thumbnails
                .iter()
                .min_by_key(|item| item["width"].as_u64().unwrap_or_default().abs_diff(350))
        } else {
            thumbnails
                .iter()
                .max_by_key(|item| item["width"].as_u64().unwrap_or_default())
        };
        if let Some(relative) = thumb.and_then(|item| item["relative_path"].as_str()) {
            let path = join_path(parent_dir(path), relative);
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
        self.runtime.spawn(async move {
            let result = async {
                const MAX_IMAGE: usize = 12 * 1024 * 1024;
                let mut response = client.get(url).send().await?.error_for_status()?;
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await? {
                    if bytes.len() + chunk.len() > MAX_IMAGE {
                        return Err("Image exceeds 12 MB".into());
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
                            && self.dialog.action == "file" =>
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

    fn network_action(&self, action: &str, argument: &str, password: &str) {
        let (action, argument) = (action.to_owned(), argument.to_owned());
        let password = (!password.is_empty()).then(|| password.to_owned());
        let tx = self.background.clone();
        self.runtime.spawn(async move {
            let result = match action.as_str() {
                "network" => Ok(()),
                "wifi-on" | "wifi-off" => network::set_wifi_enabled(action == "wifi-on").await,
                "wired-on" | "wired-off" => network::set_wired_interface_enabled(argument, action == "wired-on").await,
                "wifi-connect" => network::connect_to_wifi(argument, password).await,
                "wifi-hidden-password" => network::connect_hidden_wifi(argument, password).await,
                "wifi-forget" => network::forget_saved_wifi(argument).await,
                "wifi-scan" => network::scan_wifi_networks().await.map(|_| ()),
                _ => return,
            };
            if let Err(error) = result {
                let _ = tx.send(Background::Reply("network".into(), Err(error)));
                return;
            }
            let (ip, wifi, wired, can, status) = tokio::join!(network::get_primary_ip_address(), network::get_wifi_settings(),
                network::get_wired_settings(), network::get_canbus_settings(), network::get_network_status());
            let wifi_error = wifi.as_ref().err().cloned();
            let data = json!({"ip": ip.ok(), "wifi": wifi.ok(), "wifi-error": wifi_error, "wired": wired.ok(), "can": can.ok(), "status": status.ok()});
            let _ = tx.send(Background::Reply("network".into(), Ok(data)));
        });
    }
}

fn valid_path(value: &str) -> bool {
    !value.contains(['\r', '\n', '\0'])
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
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

            let (enabled, timeout, unlock) = config
                .lock()
                .map(|config| {
                    (
                        config["use_idle_timeout"].as_bool().unwrap_or_default(),
                        Duration::from_secs(config["idle_timeout"].as_u64().unwrap_or(900).max(1)),
                        Duration::from_millis(config["idle_unlock"].as_u64().unwrap_or(500)),
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
                            Err("No input device available to wake the screen".into()),
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
    use super::*;

    #[test]
    fn lists_moonraker_files_and_directories_in_nested_folders() {
        let directory = json!({
            "dirs": [{"dirname": "Parts"}, {"dirname": ".thumbs"}],
            "files": [{"filename": "Part A.gcode", "size": 2048, "permissions": "r"}],
        });
        let rows = directory_rows(&directory, "Models", "part", 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].argument, "Models/Parts");
        assert_eq!(rows[0].action, "folder");
        assert_eq!(rows[1].argument, "Models/Part A.gcode");
        assert_eq!(rows[1].action, "file");
        assert!(rows[1].alternate.is_empty());
    }
}
