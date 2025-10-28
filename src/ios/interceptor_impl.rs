use crate::core::interceptor::{Interceptor, InterceptorError};
use std::ffi::CString;

pub struct IosInterceptor;

impl Interceptor for IosInterceptor {
    fn hook(&self, target: usize, detour: usize) -> Result<usize, InterceptorError> {
        let mut trampoline = 0;
        let result = unsafe {
            dobby_rs::hook(
                target as *mut _,
                detour as *mut _,
                &mut trampoline as *mut _ as *mut *mut _,
            )
        };

        if result == dobby_rs::dobby_errno::DOBBY_SUCCESS {
            Ok(trampoline)
        } else {
            error!("Dobby hook failed with code: {}", result);
            Err(InterceptorError::Other)
        }
    }

    fn unhook(&self, target: usize) {
        let result = unsafe { dobby_rs::unhook(target as *mut _) };
        if result != dobby_rs::dobby_errno::DOBBY_SUCCESS {
            error!("Dobby unhook failed with code: {}", result);
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

    //

    if image_name != "UnityFramework" {
         warn!("find_symbol_by_name called for unhandled image: {}", image_name);
         return 0;
    }

    let addr = unsafe {
        libc::dlsym(libc::RTLD_DEFAULT, c_symbol_name.as_ptr())
    };

    if addr.is_null() {
        error!("Failed to find symbol '{}' in any loaded image.", symbol_name);
        0
    } else {
        info!("Found symbol '{}' at address {:p}", symbol_name, addr);
        addr as usize
    }
}
