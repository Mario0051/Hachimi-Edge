use crate::il2cpp::{api::il2cpp_resolve_icall, types::*};
use crate::core::{Hachimi, game::Region, msgpack_modifier};

type InternalGetByteArrayFn = extern "C" fn(this: *mut Il2CppObject, length: *mut i32) -> *mut u8;
extern "C" fn InternalGetByteArray(this: *mut Il2CppObject, length: *mut i32) -> *mut u8 {
    let data_ptr = get_orig_fn!(InternalGetByteArray, InternalGetByteArrayFn)(this, length);
    if data_ptr.is_null() || unsafe { *length } <= 0 { return data_ptr; }

    let config = Hachimi::instance().config.load();
    let slice = unsafe { std::slice::from_raw_parts(data_ptr, *length as usize) };

    if config.dump_msgpack {
        msgpack_modifier::dump_msgpack(slice, "R");
    }
    msgpack_modifier::read_response(slice);

    if config.unlock_live_chara {
        if let Some(modified) = msgpack_modifier::modify_response(slice) {
            unsafe { *length = modified.len() as i32; }
            let new_ptr = unsafe { libc::malloc(modified.len()) as *mut u8 };
            unsafe { std::ptr::copy_nonoverlapping(modified.as_ptr(), new_ptr, modified.len()) };
            return new_ptr;
        }
    }

    data_ptr
}

pub fn init() {
    if Hachimi::instance().game.region != Region::Korea { return; }
    let addr = il2cpp_resolve_icall(c"UnityEngine.Networking.DownloadHandler::InternalGetByteArray()".as_ptr());
    new_hook!(addr, InternalGetByteArray);
}