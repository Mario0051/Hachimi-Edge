use std::ffi::{c_void, CStr, CString};

pub struct IosSymbols;

pub unsafe fn dlsym(handle: *mut c_void, symbol: &str) -> usize {
    let c_symbol = match CString::new(symbol) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create CString for symbol {}: {}", symbol, e);
            return 0;
        }
    };

    let search_handle = if handle.is_null() {
        libc::RTLD_DEFAULT
    } else {
        handle
    };

    let addr = libc::dlsym(search_handle, c_symbol.as_ptr());

    if addr.is_null() {
        error!("dlsym failed to find symbol '{}'", symbol);
        0
    } else {
        addr as usize
    }
}

pub fn get_image_name(addr: usize) -> Option<String> {
    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::uninit();
    if unsafe { libc::dladdr(addr as *mut c_void, info.as_mut_ptr()) } == 0 {
        return None;
    }

    let info = unsafe { info.assume_init() };
    if info.dli_fname.is_null() {
        return None;
    }

    let c_str = unsafe { CStr::from_ptr(info.dli_fname) };
    Some(c_str.to_string_lossy().into_owned())
}
