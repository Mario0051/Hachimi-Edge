use crate::il2cpp::{api::il2cpp_resolve_icall, types::*};
use crate::core::{Hachimi, game::Region, msgpack_handler};

type InternalGetByteArrayFn = extern "C" fn(this: *mut Il2CppObject, length: *mut i32) -> *mut u8;
extern "C" fn InternalGetByteArray(this: *mut Il2CppObject, length: *mut i32) -> *mut u8 {
    let data_ptr = get_orig_fn!(InternalGetByteArray, InternalGetByteArrayFn)(this, length);
    if data_ptr.is_null() || unsafe { *length } <= 0 { return data_ptr; }

    let config = Hachimi::instance().config.load();
    let slice = unsafe { std::slice::from_raw_parts(data_ptr, *length as usize) };

    if config.dump_msgpack {
        msgpack_handler::dump_msgpack(slice, "R");
    }
    msgpack_handler::read_response(slice);

    data_ptr
}

pub fn init() {
    if Hachimi::instance().game.region != Region::Korea { return; }
    let addr = il2cpp_resolve_icall(c"UnityEngine.Networking.DownloadHandler::InternalGetByteArray()".as_ptr());
    new_hook!(addr, InternalGetByteArray);
}