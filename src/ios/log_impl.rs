use crate::core::Log;
use log::{Level, Log as OtherLog, Metadata, Record};
use oslog::{LogLevel, OsLog};

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
            Level::Error => LogLevel::Error,
            Level::Warn => LogLevel::Error,
            Level::Info => LogLevel::Info,
            Level::Debug => LogLevel::Debug,
            Level::Trace => LogLevel::Debug,
        };

        self.logger.log(level, &format!("{}", record.args()));
    }

    fn flush(&self) {}
}

pub fn init(level: log::LevelFilter) {
    let logger = IosLog::new();
    log::set_boxed_logger(Box::new(logger)).unwrap();
    log::set_max_level(level);

    std::panic::set_hook(Box::new(|panic_info| {
        let logger = OsLog::new("com.hachimi-edge.mod", "panic");
        logger.error(&format!("PANIC: {}", panic_info));
    }));

    info!("iOS os_log logger initialized.");
}

impl Log for IosLog {
    fn info(&self, s: &str) {
        self.logger.info("%{public}s", s);
    }
    fn warn(&self, s: &str) {
        self.logger.error("%{public}s", s);
    }
    fn error(&self, s: &str) {
        self.logger.error("%{public}s", s);
    }
}