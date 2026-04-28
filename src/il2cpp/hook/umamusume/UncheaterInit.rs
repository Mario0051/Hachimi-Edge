use crate::{core::{game::Region, Hachimi}, il2cpp::{symbols, types::*}};

extern "C" fn Init(_this: *mut Il2CppObject, _system: *mut Il2CppObject) {}

extern "C" fn InitializeUncheaterSDK(this: *mut Il2CppObject) {
    let flag_field = symbols::get_field_from_name(unsafe { (*this).klass() }, c"flag");
    symbols::set_field_value(this, flag_field, &true);
}

extern "C" fn CheckUncheaterSystem() -> bool {
    true
}

extern "C" fn setUserName(_username: *mut Il2CppString) {}

pub fn init(umamusume: *const Il2CppImage) {
    if Hachimi::instance().game.region != Region::Korea {
        return;
    }

    get_class_or_return!(umamusume, "", UncheaterInit);

    new_hook!(symbols::get_method_addr_cached(UncheaterInit, c"Init", 1), Init);
    new_hook!(symbols::get_method_addr_cached(UncheaterInit, c"InitializeUncheaterSDK", 0), InitializeUncheaterSDK);
    new_hook!(symbols::get_method_addr_cached(UncheaterInit, c"CheckUncheaterSystem", 0), CheckUncheaterSystem);
    new_hook!(symbols::get_method_addr_cached(UncheaterInit, c"setUserName", 1), setUserName);

    unsafe {
        if let Ok(uncheater_image) = symbols::get_assembly_image(c"uncheatercsd.dll") {
            if let Ok(bin_data_class) = symbols::get_class(uncheater_image, c"Uncheater", c"SystemBins64") {
                let bin_array_field = symbols::get_field_from_name(bin_data_class, c"UNCHEATER_DATA");

                let object_class = api::il2cpp_defaults.object_class;
                let array = api::il2cpp_array_new(object_class, 128);

                let byte_array = api::il2cpp_array_new(mscorlib::Byte::class(), 0);

                let array_data_ptr = (array as *mut u8).add(crate::il2cpp::types::kIl2CppSizeOfArray) as *mut *mut std::ffi::c_void;
                *array_data_ptr.add(8) = byte_array as *mut _;
                *array_data_ptr.add(9) = byte_array as *mut _;

                api::il2cpp_field_static_set_value(bin_array_field, array as *mut _);
            }
        }
    }
}