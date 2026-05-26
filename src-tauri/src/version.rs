use std::{
    fs,
    path::PathBuf,
    process::Command,
    thread,
    time::Duration,
};

const APP_NAME: &str = "moondash";
const SERVICE_NAME: &str = "moondash.service";
const INSTALLED_EXE: &str = "/usr/bin/moondash";

fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".local/state"))
        })
        .unwrap_or_else(|| std::env::temp_dir())
        .join(APP_NAME)
}

pub fn start_executable_update_watcher() {
    thread::spawn(|| loop {
        thread::sleep(Duration::from_secs(30));

        if executable_changed() {
            eprintln!("[version] installed executable changed, restarting {SERVICE_NAME}");

            match Command::new("systemctl")
                .args(["--user", "restart", SERVICE_NAME])
                .status()
            {
                Ok(status) => eprintln!("[version] restart command exited with {status}"),
                Err(err) => eprintln!("[version] failed to restart {SERVICE_NAME}: {err}"),
            }

            break;
        }
    });
}

fn executable_changed() -> bool {
    let running = fs::read_link("/proc/self/exe").ok();
    let installed = fs::canonicalize(INSTALLED_EXE).ok();

    match (running, installed) {
        (Some(running), Some(installed)) => running != installed,
        _ => false,
    }
}

pub fn check_and_restart_if_updated() {
    let current_version = env!("CARGO_PKG_VERSION");

    let dir = state_dir();
    let version_file = dir.join("last_version");

    let old_version = fs::read_to_string(&version_file)
        .unwrap_or_default()
        .trim()
        .to_string();

    if old_version == current_version {
        return;
    }

    if let Err(err) = fs::create_dir_all(&dir) {
        eprintln!("[version] failed to create state dir {:?}: {err}", dir);
        return;
    }

    if let Err(err) = fs::write(&version_file, current_version) {
        eprintln!(
            "[version] failed to write version file {:?}: {err}",
            version_file
        );
        return;
    }

    if old_version.is_empty() {
        eprintln!("[version] initialized moondash version {current_version}");
        return;
    }

    eprintln!("[version] moondash changed {old_version} -> {current_version}");

    thread::spawn(|| {
        thread::sleep(Duration::from_millis(500));

        match Command::new("systemctl")
            .args(["--user", "try-restart", SERVICE_NAME])
            .status()
        {
            Ok(status) => {
                eprintln!("[version] restart command exited with {status}");
            }
            Err(err) => {
                eprintln!("[version] failed to restart {SERVICE_NAME}: {err}");
            }
        }
    });
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}