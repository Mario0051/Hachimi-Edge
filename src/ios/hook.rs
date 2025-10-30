use crate::core::gui::Gui;
use std::ffi::c_void;
use std::sync::Mutex;
use super::titanox;

type PresentFn = unsafe extern "C" fn(this: *mut c_void, timer: *mut c_void, drawable: *mut c_void);

static mut ORIG_PRESENT: Option<PresentFn> = None;

unsafe extern "C" fn on_present(this: *mut c_void, timer: *mut c_void, drawable: *mut c_void) {
    ORIG_PRESENT.unwrap()(this, timer, drawable);

    let gui_mutex = Gui::instance_or_init("ios.menu_open_key");
    let mut gui = gui_mutex.lock().unwrap();

    super::gui_impl::render_frame(&mut gui, drawable);
}

pub fn setup_render_hook() {
    let target_fn_addr = unsafe {
        super::interceptor_impl::find_symbol_by_name(
            "UnityFramework",
            "_UnityPresentsTimerAndDrawable"
        )
    };

    if target_fn_addr == 0 {
        error!("Failed to find UnityPresentsTimerAndDrawable symbol. GUI will not be available.");
        return;
    }

    unsafe {
        let status = titanox::TXHookFunction(
            target_fn_addr as *mut c_void,
            on_present as *mut c_void,
            &mut ORIG_PRESENT as *mut _ as *mut *mut c_void,
        );

        if status != titanox::TX_SUCCESS as i32 {
            error!("Titanox hook failed for _UnityPresentsTimerAndDrawable: {}", status);
        } else {
            info!("Titanox hook successful for _UnityPresentsTimerAndDrawable.");
        }
    }
}
