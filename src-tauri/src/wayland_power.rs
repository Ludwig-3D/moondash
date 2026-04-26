use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_output::WlOutput, wl_registry},
    Connection, Dispatch, QueueHandle,
};

use wayland_protocols_wlr::output_power_management::v1::client::{
    zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1,
    zwlr_output_power_v1::{Mode, ZwlrOutputPowerV1},
};

struct WaylandPowerState {
    power_handles: Vec<ZwlrOutputPowerV1>,
}

pub fn turn_off_displays() -> Result<(), String> {
    set_outputs_power(Mode::Off)
}

pub fn turn_on_displays() -> Result<(), String> {
    set_outputs_power(Mode::On)
}

fn set_outputs_power(mode: Mode) -> Result<(), String> {
    let conn = Connection::connect_to_env()
        .map_err(|e| format!("failed to connect to Wayland: {e}"))?;

    let (globals, mut event_queue) =
        registry_queue_init::<WaylandPowerState>(&conn)
            .map_err(|e| format!("failed to init Wayland registry: {e}"))?;

    let qh = event_queue.handle();

    let mut state = WaylandPowerState {
        power_handles: Vec::new(),
    };

    let power_manager = globals
        .bind::<ZwlrOutputPowerManagerV1, _, _>(&qh, 1..=1, ())
        .map_err(|_| "compositor does not expose zwlr_output_power_manager_v1".to_string())?;

    let output = globals
        .bind::<WlOutput, _, _>(&qh, 1..=4, ())
        .map_err(|_| "compositor does not expose wl_output".to_string())?;

    let power = power_manager.get_output_power(&output, &qh, ());
    power.set_mode(mode);
    state.power_handles.push(power);

    conn.flush()
        .map_err(|e| format!("failed to flush Wayland power request: {e}"))?;

    event_queue
        .roundtrip(&mut state)
        .map_err(|e| format!("Wayland power roundtrip failed: {e}"))?;

    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandPowerState {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlOutput, ()> for WaylandPowerState {
    fn event(
        _state: &mut Self,
        _proxy: &WlOutput,
        _event: wayland_client::protocol::wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrOutputPowerManagerV1, ()> for WaylandPowerState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrOutputPowerManagerV1,
        _event: wayland_protocols_wlr::output_power_management::v1::client::zwlr_output_power_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrOutputPowerV1, ()> for WaylandPowerState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrOutputPowerV1,
        _event: wayland_protocols_wlr::output_power_management::v1::client::zwlr_output_power_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}