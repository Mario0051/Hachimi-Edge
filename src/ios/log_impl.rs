use crate::core::log::Log;
use log::{Level, Log as OtherLog, Metadata, Record};
use oslog::{sys, OsLog};

pub struct IosLog {
    logger: OsLog,
}

impl IosLog {
    pub fn new() -> IosLog {
        IosLog {
            logger: OsLog::new("com.hachimi-edge.mod", "default"),
        }
    }
}

impl OtherLog for IosLog {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let level = match record.level() {
            Level::Error => sys::OS_LOG_TYPE_ERROR,
            Level::Warn => sys::OS_LOG_TYPE_DEFAULT,
            Level::Info => sys::OS_LOG_TYPE_INFO,
            Level::Debug => sys::OS_LOG_TYPE_DEBUG,
            Level::Trace => sys::OS_LOG_TYPE_DEBUG,
        };

        self.logger.log(level, &format!("{}", record.args()));
    }

    fn flush(&self) {}
}

pub fn init(level: log.LevelFilter) {
    let logger = IosLog::new();
    log::set_boxed_logger(Box::new(logger)).unwrap();
    log::set_max_level(level);

    std::panic::set_hook(Box::new(|panic_info| {
        let logger = OsLog::new("com.hachimi-edge.mod", "panic");
        logger.log(sys::OS_LOG_TYPE_ERROR, &format!("PANIC: {}", panic_info));
    }));

    info!("iOS os_log logger initialized.");
}

impl Log for IosLog {
    fn info(&self, s: &str) {
        self.logger.log(sys::OS_LOG_TYPE_INFO, s);
    }
    fn warn(&self, s: &str) {
        self.logger.log(sys::OS_LOG_TYPE_DEFAULT, s);
    }
    fn error(&self, s: &str) {
        self.logger.log(sys::OS_LOG_TYPE_ERROR, s);
    }
}