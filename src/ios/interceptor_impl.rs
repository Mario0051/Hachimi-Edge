use crate::core::interceptor::HookHandle;
use crate::core::Error;
use std::ffi::c_void;
use std::ptr;

pub struct IosInterceptor;

pub unsafe fn hook(orig_addr: usize, hook_addr: usize) -> Result<usize, Error> {
    let trampoline = dobby_rs::hook(
        orig_addr as *mut c_void,
        hook_addr as *mut c_void
    )?;
    Ok(trampoline as usize)
}

pub unsafe fn hook_vtable(
    vtable: *mut usize,
    vtable_index: usize,
    hook_addr: usize,
) -> Result<HookHandle, Error> {
    let hook_target_ptr = vtable.add(vtable_index);

    let orig_addr = ptr::read(hook_target_ptr);

    let trampoline_addr = hook(orig_addr, hook_addr)?;

    let handle = HookHandle {
        orig_addr,
        trampoline_addr,
        hook_type: crate::core::interceptor::HookType::Vtable,
    };

    Ok(handle)
}

pub fn unhook(hook: &HookHandle) {
    if let Err(e) = unsafe { dobby_rs::unhook(hook.orig_addr as *mut c_void) } {
        error!("Failed to unhook function at {:#x}: {}", hook.orig_addr, e);
    }
}

pub fn unhook_vtable(hook: &HookHandle) {
    unhook(hook)
}

pub unsafe fn get_vtable_from_instance(instance_addr: usize) -> *mut usize {
    ptr::read(instance_addr as *const *mut usize)
}

pub unsafe fn find_symbol_by_name(_module: &str, symbol: &str) -> usize {
    let handle = ptr::null_mut(); 
    super::symbols_impl::dlsym(handle, symbol)
}