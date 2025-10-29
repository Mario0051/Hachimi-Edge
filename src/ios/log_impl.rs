use crate::core::log::Log;
use log::{LevelFilter, Log as OtherLog, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::path::PathBuf;

use objc::rc::autoreleasepool; 

use objc2_foundation::{
    NSSearchPathForDirectoriesInDomains, 
    NSSearchPathDirectory, 
    NSSearchPathDomainMask,
};

struct SimpleFileLogger {
    file: Mutex<File>,
}

impl OtherLog for SimpleFileLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(
                file,
                "[{}] {}",
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

fn get_documents_directory() -> Option<PathBuf> {
    autoreleasepool(|pool| {
        let dirs = unsafe {
            NSSearchPathForDirectoriesInDomains(
                NSSearchPathDirectory::DocumentDirectory,
                NSSearchPathDomainMask::NSUserDomainMask,
                true,
            )
        };

        let dir = dirs.first()?;
        let path_str = dir.to_string(pool);

        Some(PathBuf::from(path_str))
    })
}

pub fn init(level: log::LevelFilter) {
    let log_path = get_documents_directory()
        .map(|path| path.join("hachimi-edge.log"))
        .unwrap_or_else(|| {
            PathBuf::from("/tmp/hachimi-edge-fallback.log")
        });

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("Failed to open log file");

    let logger = SimpleFileLogger {
        file: Mutex::new(file),
    };
    log::set_boxed_logger(Box::new(logger)).unwrap();
    log::set_max_level(level);

    let panic_log_path = log_path.clone();
    std::panic::set_hook(Box::new(move |panic_info| {
        let msg = format!("PANIC: {}", panic_info);

        log::error!("{}", msg); 

        if let Ok(mut file) = OpenOptions::new().append(true).open(&panic_log_path) {
            let _ = writeln!(file, "{}", msg);
            let _ = file.flush();
        }
    }));

    log::info!("--- iOS File Logger Initialized ---");
    log::info!("Logging to: {:?}", log_path);
}

pub struct IosLog;

impl IosLog {
    pub fn new() -> IosLog {
        IosLog
    }
}

impl OtherLog for IosLog {
    fn enabled(&self, _metadata: &log::Metadata) -> bool { true }
    fn log(&self, _record: &log::Record) {}
    fn flush(&self) {}
}

impl Log for IosLog {
    fn info(&self, s: &str) { log::info!("{}", s); }
    fn warn(&self, s: &str) { log::warn!("{}", s); }
    fn error(&self, s: &str) { log::error!("{}", s); }
}