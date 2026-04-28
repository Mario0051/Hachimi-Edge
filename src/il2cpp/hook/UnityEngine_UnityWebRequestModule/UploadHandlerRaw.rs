use std::os::raw::c_void;
use crate::il2cpp::{api::il2cpp_resolve_icall, types::*};
use crate::core::{Hachimi, game::Region, msgpack_handler};

type CreateFn = extern "C" fn(this: *mut Il2CppObject, data: *mut u8, data_length: i32) -> *mut c_void;
extern "C" fn Create(this: *mut Il2CppObject, data: *mut u8, data_length: i32) -> *mut c_void {
    let config = Hachimi::instance().config.load();
    if data_length > 0 && !data.is_null() {
        let slice = unsafe { std::slice::from_raw_parts(data, data_length as usize) };
        if config.dump_msgpack && config.dump_msgpack_request {
            msgpack_handler::dump_msgpack(slice, "Q");
        }
    }
    get_orig_fn!(Create, CreateFn)(this, data, data_length)
}

pub fn init() {
    if Hachimi::instance().game.region != Region::Korea { return; }
    let addr = il2cpp_resolve_icall(c"UnityEngine.Networking.UploadHandlerRaw::Create()".as_ptr());
    new_hook!(addr, Create);
}