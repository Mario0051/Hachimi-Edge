use std::ffi::{c_void, CStr};
use std::sync::Once;
use std::thread;
use super::titanox;

static STARTUP_ONCE: Once = Once::new();

static mut REAL_DLOPEN: Option<extern "C" fn(*const i8, i32) -> *mut c_void> = None;

unsafe extern "C" fn hooked_dlopen(path: *const i8, mode: i32) -> *mut c_void {
    let handle = REAL_DLOPEN.unwrap()(path, mode);

    STARTUP_ONCE.call_once(|| {
        thread::spawn(|| {
            initialize_hachimi();
        });
    });

    handle
}

unsafe extern "C" fn hachimi_init() {
    let target_fn = libc::dlsym(libc::RTLD_NEXT, b"dlopen\0".as_ptr() as _);

    if !target_fn.is_null() {
        let status = titanox::TXHookFunction(
            target_fn,
            hooked_dlopen as *mut c_void,
            &mut REAL_DLOPEN as *mut _ as *mut *mut c_void,
        );

        if status != titanox::TX_SUCCESS as i32 {}
    }
}

fn initialize_hachimi() {
    super::log_impl::init(log::LevelFilter::Info);

    info!("Hachimi asynchronous initialization started (via hooked dlopen)...");

    crate::core::init(
        Box::new(super::log_impl::IosLog::new()),
        Box::new(super::hachimi_impl::IosHachimi),
        Box::new(super::game_impl::IosGame),
        Box::new(super::gui_impl::IosGui),
        Box::new(super::interceptor_impl::IosInterceptor),
        Box::new(super::symbols_impl::IosSymbols),
    );

    info!("Hachimi platform implementations set. Initializing Hachimi core...");
    if !crate::core::Hachimi::init() {
        error!("Failed to initialize Hachimi core");
        return;
    }

    info!("Hachimi core initialized. Initializing iOS GUI hooks...");
    super::gui_impl::init();

    info!("iOS initialization complete.");
}


#[link_section = "__DATA,__mod_init_func"]
#[used]
static CONSTRUCTOR: unsafe extern "C" fn() = {
    hachimi_init
};