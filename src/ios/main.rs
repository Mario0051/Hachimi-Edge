use ctor::ctor;
use crate::core;

use super::{
    game_impl::IosGame, gui_impl::IosGui, hachimi_impl::IosHachimi,
    interceptor_impl::IosInterceptor, log_impl::IosLog, symbols_impl::IosSymbols,
};

#[ctor]
fn entrypoint() {
    std::thread::spawn(|| {
        core::init(
            Box::new(IosLog),
            Box::new(IosHachimi),
            Box::new(IosGame),
            Box::new(IosGui),
            Box::new(IosInterceptor),
            Box::new(IosSymbols),
        );
    });
}