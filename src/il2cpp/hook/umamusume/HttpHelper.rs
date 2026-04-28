use crate::{
    core::{Hachimi, msgpack_modifier, game::Region},
    il2cpp::{symbols::get_method_addr, types::*, api::il2cpp_array_new},
};

type CompressRequestFn = extern "C" fn(data: *mut Il2CppArray) -> *mut Il2CppArray;
extern "C" fn CompressRequest(data: *mut Il2CppArray) -> *mut Il2CppArray {
    let config = Hachimi::instance().config.load();
    if let Some(slice) = get_byte_array_slice(data) {
        msgpack_modifier::broadcast_msgpack(slice, true);

        if config.dump_msgpack && config.dump_msgpack_request {
            msgpack_modifier::dump_msgpack(slice, "Q");
        }
        if config.unlock_live_chara {
            if let Some(modified) = msgpack_modifier::modify_request(slice) {
                let byte_class = crate::il2cpp::hook::mscorlib::Byte::class();
                let new_arr = il2cpp_array_new(byte_class, modified.len() as _);
                let dest = unsafe { (new_arr as *mut u8).add(crate::il2cpp::types::kIl2CppSizeOfArray) };
                unsafe { std::ptr::copy_nonoverlapping(modified.as_ptr(), dest, modified.len()) };
                return get_orig_fn!(CompressRequest, CompressRequestFn)(new_arr);
            }
        }
    }
    get_orig_fn!(CompressRequest, CompressRequestFn)(data)
}

type DecompressResponseFn = extern "C" fn(compressed: *mut Il2CppArray) -> *mut Il2CppArray;
extern "C" fn DecompressResponse(compressed: *mut Il2CppArray) -> *mut Il2CppArray {
    let data = get_orig_fn!(DecompressResponse, DecompressResponseFn)(compressed);
    let config = Hachimi::instance().config.load();

    if let Some(slice) = get_byte_array_slice(data) {
        msgpack_modifier::broadcast_msgpack(slice, false);

        if config.dump_msgpack {
            msgpack_modifier::dump_msgpack(slice, "R");
        }
        msgpack_modifier::read_response(slice);

        if config.unlock_live_chara {
            if let Some(modified) = msgpack_modifier::modify_response(slice) {
                let byte_class = crate::il2cpp::hook::mscorlib::Byte::class();
                let new_arr = il2cpp_array_new(byte_class, modified.len() as _);
                let dest = unsafe { (new_arr as *mut u8).add(crate::il2cpp::types::kIl2CppSizeOfArray) };
                unsafe { std::ptr::copy_nonoverlapping(modified.as_ptr(), dest, modified.len()) };
                return new_arr;
            }
        }
    }
    data
}

fn get_byte_array_slice<'a>(arr: *mut Il2CppArray) -> Option<&'a [u8]> {
    if arr.is_null() { return None; }
    unsafe {
        let length = (*arr).max_length as usize;
        let data_ptr = (arr as *mut u8).add(kIl2CppSizeOfArray);
        Some(std::slice::from_raw_parts(data_ptr, length))
    }
}

pub fn init(umamusume: *const Il2CppImage) {
    if Hachimi::instance().game.region == Region::Korea {
        return;
    }
    get_class_or_return!(umamusume, Gallop, HttpHelper);
    let CompressRequest_addr = get_method_addr(HttpHelper, c"CompressRequest", 1);
    let DecompressResponse_addr = get_method_addr(HttpHelper, c"DecompressResponse", 1);
    new_hook!(CompressRequest_addr, CompressRequest);
    new_hook!(DecompressResponse_addr, DecompressResponse);
}