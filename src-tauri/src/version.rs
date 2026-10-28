use std::{process::Command, thread, time::Duration};

const SERVICE_NAME: &str = "moondash.service";

/// Explicitly restart the kiosk service once Moonraker has finished updating
/// system packages which included the locally installed Moondash package.
/// Never monitor executable hashes or restart automatically at startup.
#[tauri::command]
pub fn restart_after_package_update() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if !std::path::Path::new("/usr/bin/moondash").exists() {
            return Err("Moondash is not installed at /usr/bin/moondash".into());
        }
        // Check the system service before accepting the restart.
        let check = Command::new("systemctl")
            .args(["show", "--property=LoadState", "--value", SERVICE_NAME])
            .output()
            .map_err(|e| format!("Cannot inspect Moondash service: {e}"))?;
        if !check.status.success() || String::from_utf8_lossy(&check.stdout).trim() != "loaded" {
            return Err("Moondash system service is not loaded".into());
        }
        // Use passwordless sudo: a GUI application cannot answer sudo password prompts.
        thread::spawn(|| {
            thread::sleep(Duration::from_secs(2));
            match Command::new("sudo").args(["-n", "systemctl", "restart", SERVICE_NAME]).status() {
                Ok(status) if status.success() => {},
                Ok(status) => eprintln!("[version] restart failed: {status}"),
                Err(error) => eprintln!("[version] restart failed: {error}"),
            }
        });
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    { Err("Package service restart is only supported on Linux".into()) }
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
