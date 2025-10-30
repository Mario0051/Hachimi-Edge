#[allow(dead_code, non_upper_case_globals, non_camel_case_types, non_snake_case)]
pub mod titanox {
    include!(concat!(env!("OUT_DIR"), "/titanox_bindings.rs"));

    #[allow(non_camel_case_types)]
    pub type TXStatus = ::std::os::raw::c_int;
    pub const TX_SUCCESS: TXStatus = 0;

    extern "C" {
        pub fn TXHookFunction(
            target: *mut ::std::ffi::c_void,
            replacement: *mut ::std::ffi::c_void,
            original: *mut *mut ::std::ffi::c_void,
        ) -> TXStatus;
    }
}

pub mod game_impl;
pub mod gui_impl;
pub mod hachimi_impl;
pub mod hook;
pub mod input_hook;
pub mod interceptor_impl;
pub mod log_impl;
pub mod main;
pub mod symbols_impl;
