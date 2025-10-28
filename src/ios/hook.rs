use crate::core::gui::Gui;
use std::ffi::c_void;
use once_cell::sync::OnceCell;
use std::sync::Mutex;

type PresentFn = unsafe extern "C" fn(this: *mut c_void, timer: *mut c_void, drawable: *mut c_void);

static ORIG_PRESENT: OnceCell<PresentFn> = OnceCell::new();

unsafe extern "C" fn on_present(this: *mut c_void, timer: *mut c_void, drawable: *mut c_void) {
    if let Some(orig) = ORIG_PRESENT.get() {
        orig(this, timer, drawable);
    }

    let gui_mutex = Gui::instance_or_init("ios.menu_open_key");
    let mut gui = gui_mutex.lock().unwrap();

    super::gui_impl::render_frame(&mut gui, drawable);

    /* This is the old, non-working code:
    if let Some(gui_mutex) = Gui::instance() {
        let mut gui = gui_mutex.lock().unwrap();

        super::gui_impl::render_frame(&mut gui, drawable);
    }
    */
}

pub fn setup_render_hook() {
    let target_fn_addr = unsafe {
        super::interceptor_impl::find_symbol_by_name(
            "UnityFramework",
            "UnityPresentsTimerAndDrawable"
        )
    };

    if target_fn_addr == 0 {
        error!("Failed to find UnityPresentsTimerAndDrawable symbol. GUI will not be available.");
        return;
    }

    let hachimi = crate::core::Hachimi::instance();
    match hachimi.interceptor.hook(target_fn_addr, on_present as usize) {
        Ok(trampoline) => {
            ORIG_PRESENT.set(unsafe { std::mem::transmute(trampoline) }).unwrap();
            info!("Successfully hooked render function.");
        }
        Err(e) => {
            error!("Failed to hook render function: {}", e);
        }
    }
}
