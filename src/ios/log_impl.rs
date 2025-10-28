use crate::core::Log;
use log::{Level, Log as OtherLog, Metadata, Record};
use oslog::OsLog;

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
        let msg = format!("{}", record.args());
        match record.level() {
            Level::Error => self.logger.error(&msg),
            Level::Warn => self.logger.default(&msg),
            Level::Info => self.logger.info(&msg),
            Level::Debug => self.logger.debug(&msg),
            Level::Trace => self.logger.debug(&msg),
        };
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
        self.logger.info(s);
    }
    fn warn(&self, s: &str) {
        self.logger.default(s);
    }
    fn error(&self, s: &str) {
        self.logger.error(s);
    }
}