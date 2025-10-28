

use crate::core::interceptor::{Interceptor, InterceptorError};
use std::ffi::CString;

pub struct IosInterceptor;

impl Interceptor for IosInterceptor {
    fn hook(&self, target: usize, detour: usize) -> Result<usize, InterceptorError> {
        match unsafe { dobby_rs::hook(target as *mut _, detour as *mut _) } {
            Ok(trampoline) => Ok(trampoline as usize),
            Err(e) => {
                error!("Dobby hook failed with code: {:?}", e);
                Err(InterceptorError::Other)
            }
        }
    }

    fn unhook(&self, target: usize) {
        match unsafe { dobby_rs::unhook(target as *mut _) } {
            Ok(_) => (),
            Err(e) => {
                error!("Dobby unhook failed with code: {:?}", e);
            }
        }
    }

    fn unhook_all(&self) {
        warn!("Interceptor::unhook_all() is not implemented for iOS");
    }
}

pub fn find_symbol_by_name(image_name: &str, symbol_name: &str) -> usize {
    let c_symbol_name = match CString::new(symbol_name) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create CString for symbol {}: {}", symbol_name, e);
            return 0;
        }
    };

    if image_name != "UnityFramework" {
         warn!("find_symbol_by_name called for unhandled image: {}", image_name);
    }

    let addr = unsafe {
        libc::dlsym(libc::RTLD_DEFAULT, c_symbol_name.as_ptr())
    };

    if addr.is_null() {
        error!("Failed to find symbol '{}' in any loaded image.", symbol_name);
        0
    }
