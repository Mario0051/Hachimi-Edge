use log::{self, LevelFilter, Log, Metadata, Record};

pub struct IosLog;

struct IosLogger;

impl Log for IosLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("[{} Hachimi-iOS] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: IosLogger = IosLogger;

pub fn init(filter_level: LevelFilter) {
    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(filter_level))
        .expect("Failed to initialize Hachimi logger");

    log::info!("iOS Logger initialized.");
}