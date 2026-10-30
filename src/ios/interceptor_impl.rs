use crate::core::{interceptor::HookHandle, Error};
use once_cell::sync::OnceCell;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

type MsHookFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut c_void);

enum HookBackend {
    Aot,
    Substrate { hook_fn: MsHookFn },
    Dobby,
}

static BACKEND: OnceCell<HookBackend> = OnceCell::new();
static AOT_AVAILABLE: AtomicBool = AtomicBool::new(false);

pub fn is_aot_available() -> bool {
    AOT_AVAILABLE.load(Ordering::Acquire)
}

fn get_os_major_version() -> i32 {
    unsafe {
        let mut os_version = [0u8; 32];
        let mut size = std::mem::size_of_val(&os_version);
        if libc::sysctlbyname(
            b"kern.osproductversion\0".as_ptr() as *const _,
            os_version.as_mut_ptr() as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) == 0 {
            let version_str = std::ffi::CStr::from_ptr(os_version.as_ptr() as *const _).to_string_lossy();
            if let Some(major_str) = version_str.split('.').next() {
                return major_str.parse().unwrap_or(0);
            }
        }
        0
    }
}

fn is_macos_hardware() -> bool {
    type ObjcGetClassFn = unsafe extern "C" fn(*const u8) -> *mut c_void;
    type SelRegisterNameFn = unsafe extern "C" fn(*const u8) -> *mut c_void;
    type ObjcMsgSendFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
    type ObjcMsgSendBoolFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool;
    type ClassRespondsToSelectorFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool;
    type ObjectGetClassFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;

    unsafe {
        let objc = libc::dlopen(b"/usr/lib/libobjc.A.dylib\0".as_ptr() as *const _, libc::RTLD_LAZY);
        if objc.is_null() {
            return false;
        }

        let mut is_mac = false;

        let sym_get_class = libc::dlsym(objc, b"objc_getClass\0".as_ptr() as *const _);
        let sym_sel_reg = libc::dlsym(objc, b"sel_registerName\0".as_ptr() as *const _);
        let sym_msg_send = libc::dlsym(objc, b"objc_msgSend\0".as_ptr() as *const _);
        let sym_responds = libc::dlsym(objc, b"class_respondsToSelector\0".as_ptr() as *const _);
        let sym_obj_class = libc::dlsym(objc, b"object_getClass\0".as_ptr() as *const _);

        if !sym_get_class.is_null() && !sym_sel_reg.is_null() && !sym_msg_send.is_null()
            && !sym_responds.is_null() && !sym_obj_class.is_null()
        {
            let objc_get_class: ObjcGetClassFn = std::mem::transmute(sym_get_class);
            let sel_register_name: SelRegisterNameFn = std::mem::transmute(sym_sel_reg);
            let objc_msg_send: ObjcMsgSendFn = std::mem::transmute(sym_msg_send);
            let objc_msg_send_bool: ObjcMsgSendBoolFn = std::mem::transmute(sym_msg_send);
            let class_responds_to_selector: ClassRespondsToSelectorFn = std::mem::transmute(sym_responds);
            let object_get_class: ObjectGetClassFn = std::mem::transmute(sym_obj_class);

            let cls = objc_get_class(b"NSProcessInfo\0".as_ptr());
            if !cls.is_null() {
                let sel_process_info = sel_register_name(b"processInfo\0".as_ptr());
                let process_info = objc_msg_send(cls, sel_process_info);

                if !process_info.is_null() {
                    let sel_is_mac = sel_register_name(b"isiOSAppOnMac\0".as_ptr());
                    let obj_cls = object_get_class(process_info);

                    if class_responds_to_selector(obj_cls, sel_is_mac) {
                        is_mac = objc_msg_send_bool(process_info, sel_is_mac);
                    }
                }
            }
        }

        libc::dlclose(objc);
        is_mac
    }
}

unsafe fn find_substrate_hook_fn() -> Option<MsHookFn> {
    const RTLD_DEFAULT: *mut c_void = std::ptr::null_mut::<c_void>().wrapping_sub(2) as *mut c_void;
    const HOOK_SYM: &[u8] = b"MSHookFunction\0";

    let sym = libc::dlsym(RTLD_DEFAULT, HOOK_SYM.as_ptr() as *const _);
    if !sym.is_null() {
        return Some(std::mem::transmute(sym));
    }

    const PATHS: &[&[u8]] = &[
        b"/var/jb/usr/lib/libellekit.dylib\0",
        b"/usr/lib/libsubstrate.dylib\0",
        b"/usr/lib/libhooker.dylib\0",
    ];
    for &path in PATHS {
        let handle = libc::dlopen(path.as_ptr() as *const _, libc::RTLD_LAZY | libc::RTLD_GLOBAL);
        if handle.is_null() { continue; }
        let sym = libc::dlsym(handle, HOOK_SYM.as_ptr() as *const _);
        if !sym.is_null() {
            return Some(std::mem::transmute(sym));
        }
        libc::dlclose(handle);
    }

    None
}

fn backend() -> &'static HookBackend {
    BACKEND.get_or_init(|| {
        if is_aot_available() {
            info!("iOS: using AOT hook backend");
            return HookBackend::Aot;
        }

        unsafe {
            let is_mac = is_macos_hardware();
            let major_version = get_os_major_version();

            if !is_mac && major_version >= 26 {
                info!("iOS: using Dobby for hooking");
                return HookBackend::Dobby;
            }

            if let Some(hook_fn) = find_substrate_hook_fn() {
                info!("iOS: using Substrate/Ellekit for hooking");
                return HookBackend::Substrate { hook_fn };
            }

            info!("iOS: falling back to Dobby for hooking");
            HookBackend::Dobby
        }
    })
}

pub unsafe fn init_aot_table(base_addr: usize) {
    let header = base_addr as *const u8;
    let magic = u32::from_le_bytes(*(header as *const [u8; 4]));
    if magic != 0xfeedfacf && magic != 0xfeedface {
        return;
    }

    let ncmds = u32::from_le_bytes(*(header.add(16) as *const [u8; 4]));
    let sizeofcmds = u32::from_le_bytes(*(header.add(20) as *const [u8; 4])) as usize;

    let mut text_cave_va = 0usize;
    let mut text_cap = 0usize;
    let mut data_cave_va = 0usize;
    let mut data_cap = 0usize;

    let mut offset = 32usize;
    let max_offset = 32 + sizeofcmds;

    for _ in 0..ncmds {
        if offset + 8 > max_offset { break; }
        let cmd = u32::from_le_bytes(*(header.add(offset) as *const [u8; 4]));
        let cmdsize = u32::from_le_bytes(*(header.add(offset + 4) as *const [u8; 4])) as usize;
        if cmdsize == 0 { break; }

        if cmd == 0x19 {
            let segname = std::slice::from_raw_parts(header.add(offset + 8), 16);
            let vmaddr = u64::from_le_bytes(*(header.add(offset + 24) as *const [u8; 8])) as usize;
            let vmsize = u64::from_le_bytes(*(header.add(offset + 32) as *const [u8; 8])) as usize;
            let nsects = u32::from_le_bytes(*(header.add(offset + 64) as *const [u8; 4]));

            let mut last_end = 0usize;
            let mut sec_off = offset + 72;
            for _ in 0..nsects {
                let s_addr = u64::from_le_bytes(*(header.add(sec_off + 32) as *const [u8; 8])) as usize;
                let s_size = u64::from_le_bytes(*(header.add(sec_off + 40) as *const [u8; 8])) as usize;
                if s_addr + s_size > last_end {
                    last_end = s_addr + s_size;
                }
                sec_off += 80;
            }

            let cave_va = (last_end + 15) & !15;
            let seg_end = vmaddr + vmsize;
            let cave_size = seg_end.saturating_sub(cave_va);

            if segname.starts_with(b"__TEXT\0") {
                text_cave_va = cave_va;
                text_cap = cave_size / 32;
            } else if segname.starts_with(b"__DATA\0") {
                data_cave_va = cave_va;
                data_cap = cave_size / 16;
            }
        }
        offset += cmdsize;
    }

    let hook_count = text_cap.min(data_cap);
    if hook_count == 0 || text_cave_va == 0 || data_cave_va == 0 {
        return;
    }

    let text_base = base_addr + text_cave_va;
    let data_base = base_addr + data_cave_va;

    let insn1 = *(text_base as *const u32);
    let insn2 = *((text_base + 4) as *const u32);
    let is_adrp_x16 = (insn1 & 0x9F00001F) == 0x90000010;
    let is_add_x16 = (insn2 & 0xFFC003FF) == 0x91000210;

    if !is_adrp_x16 || !is_add_x16 {
        debug!("iOS: __TEXT cave at {:#x} does not contain AOT dispatcher stubs; AOT disabled.", text_base);
        return;
    }

    info!(
        "iOS AOT Table: __TEXT cave={:#x}, __DATA cave={:#x}, capacity={} hooks",
        text_base, data_base, hook_count
    );

    for i in 0..hook_count {
        let disp_addr = text_base + (i * 32);
        let tramp_addr = disp_addr + 16;
        let slot = (data_base + (i * 16)) as *mut usize;

        *slot = tramp_addr;
        *(slot.add(1)) = tramp_addr;
    }

    AOT_AVAILABLE.store(true, Ordering::Release);
}

unsafe fn hook_aot(orig_addr: usize, hook_addr: usize) -> Result<usize, Error> {
    let insn1 = *(orig_addr as *const u32);

    if (insn1 & 0xFC000000) == 0x14000000 {
        let mut offset = (insn1 & 0x03FFFFFF) as i32;
        if (offset & 0x02000000) != 0 {
            offset |= -0x04000000;
        }
        let disp_addr = (orig_addr as i64 + (offset as i64 * 4)) as usize;

        let disp_insn1 = *(disp_addr as *const u32);
        let disp_insn2 = *((disp_addr + 4) as *const u32);

        let is_adrp_x16 = (disp_insn1 & 0x9F00001F) == 0x90000010;
        let is_add_x16 = (disp_insn2 & 0xFFC003FF) == 0x91000210;

        if is_adrp_x16 && is_add_x16 {
            let immlo = (disp_insn1 >> 29) & 3;
            let immhi = (disp_insn1 >> 5) & 0x7FFFF;
            let imm = (immhi << 2) | immlo;

            let imm_sign_ext = if (imm & 0x100000) != 0 {
                (imm as i64) | -0x200000
            } else {
                imm as i64
            };

            let page_offset = imm_sign_ext * 4096;
            let pc_page = (disp_addr as u64) & !0xFFF;
            let data_page = (pc_page as i64 + page_offset) as u64;

            let add_imm = (disp_insn2 >> 10) & 0xFFF;
            let data_ptr_addr = data_page + add_imm as u64;

            let hook_ptr = data_ptr_addr as *mut usize;
            let tramp_ptr = (data_ptr_addr + 8) as *const usize;

            let tramp_addr = *tramp_ptr;
            *hook_ptr = hook_addr;

            info!("iOS AOT Hook successful! Orig: {:#x} -> Hook: {:#x}, Trampoline: {:#x}", orig_addr, hook_addr, tramp_addr);
            Ok(tramp_addr)
        } else {
            Err(Error::HookingError(format!("Dispatcher at {:#x} is invalid", disp_addr)))
        }
    } else {
        Err(Error::HookingError(format!("Function at {:#x} was not AOT patched", orig_addr)))
    }
}

unsafe fn hook_substrate(hook_fn: MsHookFn, orig_addr: usize, hook_addr: usize) -> Result<usize, Error> {
    let mut trampoline: *mut c_void = std::ptr::null_mut();
    hook_fn(orig_addr as *mut c_void, hook_addr as *mut c_void, &mut trampoline);
    if trampoline.is_null() {
        Err(Error::HookingError("MSHookFunction returned null trampoline".into()))
    } else {
        Ok(trampoline as usize)
    }
}

unsafe fn hook_dobby(orig_addr: usize, hook_addr: usize) -> Result<usize, Error> {
    Ok(dobby_rs::hook(orig_addr as *mut c_void, hook_addr as *mut c_void)? as usize)
}

pub unsafe fn hook(orig_addr: usize, hook_addr: usize) -> Result<usize, Error> {
    match backend() {
        HookBackend::Aot => {
            match hook_aot(orig_addr, hook_addr) {
                Ok(tramp) => Ok(tramp),
                Err(aot_err) => {
                    warn!("AOT hook failed ({}); attempting dynamic hook fallback...", aot_err);
                    if let Some(hook_fn) = find_substrate_hook_fn() {
                        hook_substrate(hook_fn, orig_addr, hook_addr)
                    } else {
                        hook_dobby(orig_addr, hook_addr)
                    }
                }
            }
        }
        HookBackend::Substrate { hook_fn } => {
            hook_substrate(*hook_fn, orig_addr, hook_addr)
        }
        HookBackend::Dobby => {
            hook_dobby(orig_addr, hook_addr)
        }
    }
}

unsafe fn unhook_aot(hook: &HookHandle) -> Result<(), Error> {
    let orig_addr = hook.orig_addr;
    let insn1 = *(orig_addr as *const u32);

    if (insn1 & 0xFC000000) == 0x14000000 {
        let mut offset = (insn1 & 0x03FFFFFF) as i32;
        if (offset & 0x02000000) != 0 {
            offset |= -0x04000000;
        }
        let disp_addr = (orig_addr as i64 + (offset as i64 * 4)) as usize;

        let disp_insn1 = *(disp_addr as *const u32);
        let disp_insn2 = *((disp_addr + 4) as *const u32);

        let is_adrp_x16 = (disp_insn1 & 0x9F00001F) == 0x90000010;
        let is_add_x16 = (disp_insn2 & 0xFFC003FF) == 0x91000210;

        if is_adrp_x16 && is_add_x16 {
            let immlo = (disp_insn1 >> 29) & 3;
            let immhi = (disp_insn1 >> 5) & 0x7FFFF;
            let imm = (immhi << 2) | immlo;

            let imm_sign_ext = if (imm & 0x100000) != 0 {
                (imm as i64) | -0x200000
            } else {
                imm as i64
            };

            let page_offset = imm_sign_ext * 4096;
            let pc_page = (disp_addr as u64) & !0xFFF;
            let data_page = (pc_page as i64 + page_offset) as u64;

            let add_imm = (disp_insn2 >> 10) & 0xFFF;
            let data_ptr_addr = data_page + add_imm as u64;

            let hook_ptr = data_ptr_addr as *mut usize;
            let tramp_ptr = (data_ptr_addr + 8) as *const usize;

            *hook_ptr = *tramp_ptr;
            info!("iOS AOT Unhook successful! Orig: {:#x}", orig_addr);
            return Ok(());
        }
    }

    let _ = dobby_rs::unhook(hook.orig_addr as *mut c_void);
    Ok(())
}

pub unsafe fn unhook(hook: &HookHandle) -> Result<(), Error> {
    match backend() {
        HookBackend::Aot => {
            unhook_aot(hook)
        }
        HookBackend::Substrate { .. } => {
            Ok(())
        }
        HookBackend::Dobby => {
            dobby_rs::unhook(hook.orig_addr as *mut c_void)?;
            Ok(())
        }
    }
}

impl From<dobby_rs::DobbyHookError> for Error {
    fn from(e: dobby_rs::DobbyHookError) -> Self {
        Error::HookingError(e.to_string())
    }
}

pub unsafe fn find_symbol_by_name(module: &str, symbol: &str) -> Result<usize, Error> {
    if let Some(addr) = dobby_rs::resolve_symbol(module, symbol) {
        return Ok(addr as usize);
    }

    if let Ok(sym_c) = std::ffi::CString::new(symbol) {
        let addr = libc::dlsym(libc::RTLD_DEFAULT, sym_c.as_ptr());
        if !addr.is_null() { return Ok(addr as usize); }

        let underscored = format!("_{}", symbol);
        if let Ok(sym_under) = std::ffi::CString::new(underscored) {
            let addr_under = libc::dlsym(libc::RTLD_DEFAULT, sym_under.as_ptr());
            if !addr_under.is_null() { return Ok(addr_under as usize); }
        }
    }

    Err(Error::SymbolNotFound(module.to_owned(), symbol.to_owned()))
}

pub unsafe fn get_vtable_from_instance(_instance_addr: usize) -> *mut usize {
    unimplemented!("vtable hooking not used on iOS")
}

pub unsafe fn hook_vtable(_vtable: *mut usize, _vtable_index: usize, _hook_addr: usize) -> Result<HookHandle, Error> {
    unimplemented!("vtable hooking not used on iOS")
}

pub unsafe fn unhook_vtable(_hook: &HookHandle) -> Result<(), Error> {
    unimplemented!("vtable hooking not used on iOS")
}