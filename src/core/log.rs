use crate::log_impl;

pub trait Log: Send + Sync {
    fn info(&self, s: &str);
    fn warn(&self, s: &str);
    fn error(&self, s: &str);
}

pub fn init(debug_mode: bool) {
    let filter_level = if debug_mode {
        log::LevelFilter::Debug
    }
    else {
        log::LevelFilter::Info
    };

    log_impl::init(filter_level);
}