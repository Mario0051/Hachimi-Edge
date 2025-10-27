use std::ffi::CString;
use std::os::raw::c_void;

pub struct IosSymbols;

pub fn dlsym(handle: *mut c_void, name: &str) -> usize {
    let c_name = match CString::new(name) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    unsafe {
        libc::dlsym(handle, c_name.as_ptr()) as usize
    }
}