use chrono::{DateTime, Local, NaiveDate, SecondsFormat};
use flate2::{Compression, write::GzEncoder};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const ACTIVE: &str = "moondash.log";
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30);

pub struct Change {
    pub label: String,
    before: Option<String>,
    after: String,
}

impl Change {
    pub fn new(label: impl Into<String>, before: Option<String>, after: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            before,
            after: after.into(),
        }
    }

    pub fn number(
        label: impl Into<String>,
        before: Option<f64>,
        after: f64,
        precision: usize,
        unit: &str,
    ) -> Self {
        let format = |value: f64| format!("{value:.precision$}{unit}");
        Self::new(label, before.map(format), format(after))
    }

    fn message(&self) -> Option<String> {
        let before = self.before.as_deref().unwrap_or("unknown");
        (self.before.as_deref() != Some(self.after.as_str()))
            .then(|| format!("{}: {before} -> {}", self.label, self.after))
    }

    pub fn record(&self) {
        if let Some(message) = self.message() {
            log::info!(target: "moondash::settings", "{message}");
        }
    }
}

enum Command {
    Entry(String),
    Flush(Sender<()>),
    Stop,
}

struct Logger(Sender<Command>);

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        (metadata.target() == "moondash" || metadata.target().starts_with("moondash::"))
            && metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        use std::fmt::Write as _;
        let mut line = format!(
            "{} {:<5} pid={} [{}] ",
            Local::now().to_rfc3339_opts(SecondsFormat::Millis, false),
            record.level(),
            std::process::id(),
            record.target().trim_start_matches("moondash::"),
        );
        for ch in record.args().to_string().chars() {
            if ch.is_control() {
                let _ = write!(line, "{}", ch.escape_default());
            } else {
                line.push(ch);
            }
        }
        if let Err(error) = self.0.send(Command::Entry(line))
            && let Command::Entry(line) = error.0
        {
            let _ = writeln!(io::stderr(), "{line}");
        }
    }

    fn flush(&self) {
        let (ack, flushed) = mpsc::channel();
        if self.0.send(Command::Flush(ack)).is_ok() {
            let _ = flushed.recv_timeout(Duration::from_secs(2));
        }
    }
}

pub struct Guard {
    sender: Sender<Command>,
    writer: Option<JoinHandle<()>>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Stop);
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

pub fn init() -> Option<Guard> {
    let home = std::env::var_os("HOME").filter(|value| !value.is_empty());
    let directory = home.map(|home| PathBuf::from(home).join("printer_data/logs"));
    let (sender, receiver) = mpsc::channel();
    let writer = thread::Builder::new()
        .name("moondash-log".into())
        .spawn(move || write_logs(directory.as_deref(), receiver))
        .map_err(|error| eprintln!("moondash: could not start log writer: {error}"))
        .ok()?;
    let guard = Guard {
        sender,
        writer: Some(writer),
    };
    if log::set_boxed_logger(Box::new(Logger(guard.sender.clone()))).is_err() {
        eprintln!("moondash: could not register log writer");
        return None;
    }
    log::set_max_level(log::LevelFilter::Info);
    Some(guard)
}

fn write_logs(directory: Option<&Path>, receiver: Receiver<Command>) {
    let mut console = io::stderr();
    if directory.is_none() {
        let _ = writeln!(console, "moondash: HOME is unset, writing logs to stderr");
    }
    let mut last_maintenance = None::<Instant>;
    let mut last_error = None::<String>;
    loop {
        let line = match receiver.recv_timeout(MAINTENANCE_INTERVAL) {
            Ok(Command::Entry(line)) => Some(line),
            Ok(Command::Flush(ack)) => {
                let _ = ack.send(());
                continue;
            }
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => None,
        };
        let maintenance =
            last_maintenance.is_none_or(|last| last.elapsed() >= MAINTENANCE_INTERVAL);
        if maintenance {
            last_maintenance = Some(Instant::now());
        }
        if let Some(directory) = directory {
            match write_log(
                directory,
                Local::now().date_naive(),
                line.as_deref(),
                maintenance,
            ) {
                Ok(()) => {
                    if maintenance {
                        last_error = None;
                    }
                }
                Err(error) => {
                    let error = error.to_string();
                    if last_error.as_ref() != Some(&error) {
                        let _ = writeln!(
                            console,
                            "moondash: log error in {}: {error}",
                            directory.display()
                        );
                        last_error = Some(error);
                    }
                }
            }
        }
        if let Some(line) = line {
            let _ = writeln!(console, "{line}");
        }
    }
}

fn write_log(
    directory: &Path,
    today: NaiveDate,
    line: Option<&str>,
    maintenance: bool,
) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(directory.join(".moondash.log.lock"))?;
    lock.lock()?;

    let active = directory.join(ACTIVE);
    let mut rotated = false;
    if let Ok(metadata) = fs::metadata(&active) {
        let date = DateTime::<Local>::from(metadata.modified()?).date_naive();
        if metadata.len() > 0 && date != today {
            fs::rename(&active, next_archive_name(directory, date))?;
            rotated = true;
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(active)?;
    if let Some(line) = line {
        // One write buffer keeps each entry together even across separate instances.
        file.write_all(format!("{line}\n").as_bytes())?;
    }
    if rotated || maintenance {
        archive_pending(directory)?;
    }
    Ok(())
}

fn next_archive_name(directory: &Path, date: NaiveDate) -> PathBuf {
    for sequence in 0.. {
        let suffix = if sequence == 0 {
            String::new()
        } else {
            format!("-{sequence}")
        };
        let path = directory.join(format!("moondash-{date}{suffix}.log"));
        if !path.exists() && !path.with_extension("tar.gz").exists() {
            return path;
        }
    }
    unreachable!()
}

fn archive_pending(directory: &Path) -> io::Result<()> {
    let mut result = Ok(());
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(stamp) = name
            .to_str()
            .and_then(|name| name.strip_prefix("moondash-"))
            .and_then(|name| name.strip_suffix(".log"))
        else {
            continue;
        };
        let Some(date) = stamp.get(..10) else {
            continue;
        };
        let suffix = &stamp[10..];
        if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err()
            || (!suffix.is_empty()
                && !suffix.strip_prefix('-').is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
                }))
            || !entry.file_type()?.is_file()
        {
            continue;
        }
        if let Err(error) = archive_file(directory, &entry.path(), Path::new(&name)) {
            result = Err(error);
        }
    }
    result
}

fn archive_file(directory: &Path, source: &Path, name: &Path) -> io::Result<()> {
    let archive = source.with_extension("tar.gz");
    if !archive.exists() {
        let mut temp = tempfile::NamedTempFile::new_in(directory)?;
        let encoder = GzEncoder::new(temp.as_file_mut(), Compression::default());
        let mut tar = tar::Builder::new(encoder);
        tar.append_file(name, &mut File::open(source)?)?;
        tar.into_inner()?.finish()?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(&archive)
            .map_err(|error| error.error)?;
    }
    File::open(directory)?.sync_all()?;
    fs::remove_file(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::{fs::FileTimes, io::Read, time::SystemTime};

    #[test]
    fn formats_setting_transitions_and_skips_unchanged_values() {
        assert_eq!(
            Change::number("Flow", Some(100.), 105., 0, "%")
                .message()
                .as_deref(),
            Some("Flow: 100% -> 105%")
        );
        assert_eq!(
            Change::number("Flow", Some(100.), 100., 0, "%").message(),
            None
        );
    }

    #[test]
    fn rolls_the_previous_day_and_recovers_uncompressed_logs() {
        let directory = tempfile::tempdir().unwrap();
        let directory = directory.path();
        let active = directory.join(ACTIVE);
        let now = Local::now();
        let yesterday = now.date_naive().pred_opt().unwrap();
        let previous_time = SystemTime::now() - Duration::from_secs(24 * 60 * 60 + 3600);
        let previous_date = DateTime::<Local>::from(previous_time).date_naive();
        fs::write(&active, "previous entry\n").unwrap();
        File::options()
            .write(true)
            .open(&active)
            .unwrap()
            .set_times(FileTimes::new().set_modified(previous_time))
            .unwrap();

        let recovered = format!("moondash-{}-1", yesterday.pred_opt().unwrap());
        fs::write(
            directory.join(format!("{recovered}.log")),
            "recovered entry\n",
        )
        .unwrap();
        write_log(directory, now.date_naive(), None, true).unwrap();
        assert_eq!(fs::read_to_string(&active).unwrap(), "");
        write_log(directory, now.date_naive(), Some("current entry"), false).unwrap();

        assert_eq!(fs::read_to_string(active).unwrap(), "current entry\n");
        for (stem, expected) in [
            (format!("moondash-{previous_date}"), "previous entry\n"),
            (recovered, "recovered entry\n"),
        ] {
            let log_name = format!("{stem}.log");
            let compressed = File::open(directory.join(format!("{stem}.tar.gz"))).unwrap();
            let mut archive = tar::Archive::new(GzDecoder::new(compressed));
            let mut entries = archive.entries().unwrap();
            let mut entry = entries.next().unwrap().unwrap();
            assert_eq!(entry.path().unwrap(), Path::new(&log_name));
            let mut contents = String::new();
            entry.read_to_string(&mut contents).unwrap();
            assert_eq!(contents, expected);
            assert!(entries.next().is_none());
            assert!(!directory.join(log_name).exists());
        }
    }
}
