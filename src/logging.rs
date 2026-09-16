use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use windows::Win32::System::SystemInformation::GetLocalTime;

static LOGGER: std::sync::OnceLock<Mutex<LogWriter>> = std::sync::OnceLock::new();

struct LogWriter {
    file: File,
    bytes_written: u64,
}

const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024; // 5MB

pub fn log_path() -> PathBuf {
    let mut path = PathBuf::new();
    if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
        path.push(appdata);
        path.push("Zoomify");
        path.push("zoomify.log");
    } else {
        path.push("zoomify.log");
    }
    path
}

fn open_log_file() -> std::io::Result<(File, u64)> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if let Ok(metadata) = std::fs::metadata(&path)
        && metadata.len() > MAX_LOG_SIZE
    {
        let mut old_path = path.clone();
        old_path.set_extension("log.old");
        let _ = std::fs::rename(&path, &old_path);
    }

    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let size = file.metadata()?.len();
    Ok((file, size))
}

pub fn init() {
    if let Ok((file, size)) = open_log_file() {
        let _ = LOGGER.set(Mutex::new(LogWriter {
            file,
            bytes_written: size,
        }));
    }
}

pub fn log(level: &str, message: &str) {
    if let Some(mutex) = LOGGER.get()
        && let Ok(mut writer) = mutex.lock()
    {
        let st = unsafe { GetLocalTime() };

        let log_line = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} [{}] {}\n",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, level, message
        );

        let bytes = log_line.as_bytes();
        if writer.file.write_all(bytes).is_ok() {
            let _ = writer.file.flush();
            writer.bytes_written += bytes.len() as u64;

            if writer.bytes_written > MAX_LOG_SIZE
                && let Ok((new_file, new_size)) = open_log_file()
            {
                writer.file = new_file;
                writer.bytes_written = new_size;
            }
        }
    }
}

// Convenience macros
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::logging::log("INFO", &format!($($arg)*)) };
}
#[allow(unused_macros)]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::logging::log("WARN", &format!($($arg)*)) };
}
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::logging::log("ERROR", &format!($($arg)*)) };
}
#[allow(unused_imports)]
pub(crate) use {log_error, log_info, log_warn};
