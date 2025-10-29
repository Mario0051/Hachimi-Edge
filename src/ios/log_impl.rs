use crate::core::log::Log;
use log::{LevelFilter, Log as OtherLog, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::path::PathBuf;

use objc2::rc::autoreleasepool; 

use objc2_foundation::{
    NSFileManager,
    NSSearchPathDirectory, 
    NSSearchPathDomainMask,
    NSString,
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
    autoreleasepool(|_pool| {
        let file_manager = NSFileManager::defaultManager();

        let urls = file_manager.URLsForDirectory_inDomains(
            NSSearchPathDirectory::DocumentDirectory,
            NSSearchPathDomainMask::UserDomainMask,
        );

        let dir_url = urls.firstObject()?;

        let path_string = dir_url.path()?;

        let path_str = path_string.to_string(); 

        Some(PathBuf::from(path_str))
    })
}

pub fn init(level: log::LevelFilter) {
    let docs_dir = get_documents_directory()
        .expect("Hachimi PANIC: Could not find 'Documents' directory via NSFileManager.");

    let log_path = docs_dir.join("hachimi-edge.log");

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect(&format!("Hachimi PANIC: Failed to open log file at: {:?}", log_path));

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

    log::info!("--- iOS File Logger Initialized (NSFileManager) ---");
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