use crate::core::gui::Gui;
use std::ffi::{c_void, CString};
use std::sync::Mutex;
use objc::{msg_send, sel, sel_impl};
use objc::runtime::Class;
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
    unsafe {
        let titanox_hook_class = Class::get("TitanoxHook").unwrap();

        let symbol_name = CString::new("_UnityPresentsTimerAndDrawable").unwrap();
        let lib_name = CString::new("UnityFramework").unwrap();

        let _: () = msg_send![titanox_hook_class,
            hookStaticFunction: symbol_name.as_ptr(),
            withReplacement: on_present as *mut c_void,
            inLibrary: lib_name.as_ptr(),
            outOldFunction: &mut ORIG_PRESENT as *mut _ as *mut *mut c_void
        ];

        if ORIG_PRESENT.is_some() {
            info!("Titanox hook successful for _UnityPresentsTimerAndDrawable.");
        } else {
            error!("Titanox hook failed for _UnityPresentsTimerAndDrawable. ORIG_PRESENT is null.");
        }
    }
}
