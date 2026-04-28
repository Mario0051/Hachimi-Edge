use crate::{
    core::{Hachimi, msgpack_handler, game::Region},
    il2cpp::{symbols::get_method_addr, types::*},
};

type CompressRequestFn = extern "C" fn(data: *mut Il2CppArray) -> *mut Il2CppArray;
extern "C" fn CompressRequest(data: *mut Il2CppArray) -> *mut Il2CppArray {
    let config = Hachimi::instance().config.load();
    if let Some(slice) = get_byte_array_slice(data) {
        msgpack_handler::broadcast_msgpack(slice, true);

        if config.dump_msgpack && config.dump_msgpack_request {
            msgpack_handler::dump_msgpack(slice, "Q");
        }

    }
    get_orig_fn!(CompressRequest, CompressRequestFn)(data)
}

type DecompressResponseFn = extern "C" fn(compressed: *mut Il2CppArray) -> *mut Il2CppArray;
extern "C" fn DecompressResponse(compressed: *mut Il2CppArray) -> *mut Il2CppArray {
    let data = get_orig_fn!(DecompressResponse, DecompressResponseFn)(compressed);
    let config = Hachimi::instance().config.load();

    if let Some(slice) = get_byte_array_slice(data) {
        msgpack_handler::broadcast_msgpack(slice, false);

        if config.dump_msgpack {
            msgpack_handler::dump_msgpack(slice, "R");
        }
        msgpack_handler::read_response(slice);
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