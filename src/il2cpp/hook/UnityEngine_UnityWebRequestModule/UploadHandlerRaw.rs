use std::os::raw::c_void;
use crate::il2cpp::{api::il2cpp_resolve_icall, types::*};
use crate::core::{Hachimi, game::Region, msgpack_modifier};

type CreateFn = extern "C" fn(this: *mut Il2CppObject, data: *mut u8, data_length: i32) -> *mut c_void;
extern "C" fn Create(this: *mut Il2CppObject, data: *mut u8, data_length: i32) -> *mut c_void {
    let config = Hachimi::instance().config.load();
    if data_length > 0 && !data.is_null() {
        let slice = unsafe { std::slice::from_raw_parts(data, data_length as usize) };
        if config.dump_msgpack && config.dump_msgpack_request {
            msgpack_modifier::dump_msgpack(slice, "Q");
        }
        if config.unlock_live_chara {
            if let Some(modified) = msgpack_modifier::modify_request(slice) {
                let new_ptr = unsafe { libc::malloc(modified.len()) as *mut u8 };
                unsafe { std::ptr::copy_nonoverlapping(modified.as_ptr(), new_ptr, modified.len()) };
                return get_orig_fn!(Create, CreateFn)(this, new_ptr, modified.len() as i32);
            }
        }
    }
    get_orig_fn!(Create, CreateFn)(this, data, data_length)
}

pub fn init() {
    if Hachimi::instance().game.region != Region::Korea { return; }
    let addr = il2cpp_resolve_icall(c"UnityEngine.Networking.UploadHandlerRaw::Create()".as_ptr());
    new_hook!(addr, Create);
}