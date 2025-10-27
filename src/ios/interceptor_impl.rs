use crate::core::interceptor::HookHandle;
use crate::core::Error;

pub struct IosInterceptor;

pub fn unhook(_hook: &HookHandle) {
    todo!()
}

pub fn unhook_vtable(_hook: &HookHandle) {
    todo!()
}

pub unsafe fn hook(_orig_addr: usize, _hook_addr: usize) -> Result<usize, Error> {
    todo!()
}

pub unsafe fn hook_vtable(_vtable: *mut usize, _vtable_index: usize, _hook_addr: usize) -> Result<HookHandle, Error> {
    todo!()
}

pub unsafe fn get_vtable_from_instance(_instance_addr: usize) -> *mut usize {
    todo!()
}

pub unsafe fn find_symbol_by_name(_module: &str, _symbol: &str) -> usize {
    todo!()
}