use std::{collections::hash_map, sync::Mutex};

use fnv::FnvHashMap;

use crate::interceptor_impl;

use super::Error;

#[derive(Default)]
pub struct Interceptor {
    hook_map: Mutex<FnvHashMap<usize, HookHandle>>,
    vtable_hooks: Mutex<FnvHashMap<usize, FnvHashMap<usize, HookHandle>>>,
}

#[derive(Default, Clone, Copy)]
pub struct HookHandle {
    pub orig_addr: usize,
    pub trampoline_addr: usize,
    pub hook_type: HookType,
}

impl HookHandle {
    unsafe fn unhook(&self) -> Result<(), Error> {
        match self.hook_type {
            HookType::Function => Ok(interceptor_impl::unhook(self)),
            HookType::Vtable => Ok(interceptor_impl::unhook_vtable(self)),
        }
    }
}

#[derive(Default, Clone, Copy)]
pub enum HookType {
    #[default]
    Function,
    Vtable,
}

impl Interceptor {
    pub fn hook(&self, orig_addr: usize, hook_addr: usize) -> Result<usize, Error> {
        match self.hook_map.lock().unwrap().entry(hook_addr) {
            hash_map::Entry::Occupied(e) => Ok(e.get().trampoline_addr),
            hash_map::Entry::Vacant(e) => {
                let trampoline_addr = unsafe { interceptor_impl::hook(orig_addr, hook_addr)? };
                e.insert(
                    HookHandle {
                        orig_addr,
                        trampoline_addr,
                        hook_type: HookType::Function
                    }
                );
                Ok(trampoline_addr)
            },
        }
    }

    pub fn hook_vtable(&self, vtable: *mut usize, vtable_index: usize, hook_addr: usize) -> Result<usize, Error> {
        let vtable_addr = vtable as usize;

        if self.vtable_hooks.lock().unwrap().get(&vtable_addr).map_or(false, |h| h.contains_key(&vtable_index)) {
             return Err(Error::AlreadyHooked);
        }

        let hook_handle = unsafe { interceptor_impl::hook_vtable(vtable, vtable_index, hook_addr)? };
        let trampoline_addr = hook_handle.trampoline_addr;
        let mut vtable_hooks = self.vtable_hooks.lock().unwrap();
        let vtable_hook = vtable_hooks.entry(vtable_addr).or_default();
        let e = vtable_hook.entry(vtable_index).or_default();
        *e = hook_handle;
        Ok(trampoline_addr)
    }

    pub fn get_trampoline_addr(&self, hook_addr: usize) -> usize {
        if let Some(hook) = self.hook_map.lock().unwrap().get(&hook_addr) {
            hook.trampoline_addr
        }
        else {
            warn!("Attempted to get invalid hook: {}", hook_addr);
            0
        }
    }

    pub fn unhook(&self, hook_addr: usize) -> Option<HookHandle> {
        let hook = self.hook_map.lock().unwrap().remove(&hook_addr)?;
        if let Err(e) = unsafe { hook.unhook() } {
            error!("Failed to unhook {}: {}", hook.orig_addr, e);
        }

        Some(hook)
    }

    pub fn unhook_all(&self) {
        for (_, hook) in self.hook_map.lock().unwrap().drain() {
            if let Err(e) = unsafe { hook.unhook() } {
                error!("Failed to unhook {}: {}", hook.orig_addr, e);
            }
        }
    }

    pub fn get_vtable_from_instance(instance_addr: usize) -> *mut usize {
        unsafe { interceptor_impl::get_vtable_from_instance(instance_addr) }
    }

    pub fn find_symbol_by_name(module: &str, symbol: &str) -> Result<usize, Error> {
        unsafe { Ok(interceptor_impl::find_symbol_by_name(module, symbol)) }
    }
}

macro_rules! get_orig_fn {
    ($hook:ident, $type:tt) => (
        unsafe { std::mem::transmute::<usize, $type>(crate::core::Hachimi::instance().interceptor.get_trampoline_addr($hook as usize)) }
    )
}