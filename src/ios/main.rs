use std::ffi::{c_void, CStr};
use std::sync::Once;

static STARTUP_ONCE: Once = Once::new();

#[no_mangle]
pub unsafe extern "C" fn dlopen(path: *const i8, mode: i32) -> *mut c_void {
    let real_dlopen: extern "C" fn(*const i8, i32) -> *mut c_void =
        std::mem::transmute(libc::dlsym(libc::RTLD_NEXT, b"dlopen\0".as_ptr() as _));

    let handle = real_dlopen(path, mode);

    if !path.is_null() {
        let path_str = CStr::from_ptr(path).to_string_lossy();

        if path_str.contains("UnityFramework") {
            STARTUP_ONCE.call_once(|| {
                std::thread::spawn(initialize_hachimi);
            });
        }
    }

    handle
}

fn initialize_hachimi() {
    super::log_impl::init(log::LevelFilter::Info);
    crate::core::init(
        Box::new(super::log_impl::IosLog),
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

    info!("Hachimi core initialized. Setting up render hook...");
    super::hook::setup_render_hook();

    info!("iOS initialization complete.");
}
