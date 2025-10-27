pub mod hachimi;
pub use hachimi::Hachimi;

mod error;
pub use error::Error;

pub mod default {
    pub use ::std::default::Default;
}

pub mod game;
pub mod ext;
pub mod template;

pub mod gui;
pub use gui::Gui;

pub mod plurals;
mod template_filters;

#[macro_use] pub mod interceptor;
pub use interceptor::Interceptor;

pub mod utils;
pub mod http;
pub mod tl_repo;
pub mod log;
mod ipc;

mod sugoi_client;
pub use sugoi_client::SugoiClient;

pub mod plugin_api;

pub fn init(
    _log: Box<dyn std::any::Any>,
    _hachimi: Box<dyn std::any::Any>,
    _game: Box<dyn std::any::Any>,
    _gui: Box<dyn std::any::Any>,
    _interceptor: Box<dyn std::any::Any>,
    _symbols: Box<dyn std::any::Any>,
) {
    println!("[Hachimi] core::init called!");
}