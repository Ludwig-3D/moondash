use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_output::WlOutput, wl_registry},
    Connection, Dispatch, Proxy, QueueHandle,
};

use wayland_protocols_wlr::output_power_management::v1::client::{
    zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1,
    zwlr_output_power_v1::{Mode, ZwlrOutputPowerV1},
};

struct WaylandPowerState {
    outputs: Vec<WlOutput>,
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
        outputs: Vec::new(),
        power_handles: Vec::new(),
    };

    event_queue
        .roundtrip(&mut state)
        .map_err(|e| format!("Wayland registry roundtrip failed: {e}"))?;

    let power_manager = globals
        .bind::<ZwlrOutputPowerManagerV1, _, _>(&qh, 1..=1, ())
        .map_err(|_| "compositor does not expose zwlr_output_power_manager_v1".to_string())?;

    if state.outputs.is_empty() {
        return Err("no Wayland outputs found".to_string());
    }

    for output in &state.outputs {
        let power = power_manager.get_output_power(output, &qh, ());
        power.set_mode(mode);
        state.power_handles.push(power);
    }

    conn.flush()
        .map_err(|e| format!("failed to flush Wayland requests: {e}"))?;

    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandPowerState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == WlOutput::interface().name {
                let output = registry.bind::<WlOutput, _, _>(name, version.min(4), qh, ());
                state.outputs.push(output);
            }
        }
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