use std::path::PathBuf;

/// Returns the path for the Hachimi log file, inside the game sandbox Documents dir.
pub fn get_log_path() -> PathBuf {
    crate::ios::game_impl::get_data_dir("").join("hachimi.log")
}

pub fn set_clipboard_text(text: &str) -> bool {
    use objc2::runtime::AnyClass;
    use objc2::msg_send;
    unsafe {
        if let Some(cls) = AnyClass::get("UIPasteboard") {
            let pb: *mut objc2::runtime::AnyObject = msg_send![cls, generalPasteboard];
            if !pb.is_null() {
                if let Some(ns_str_cls) = AnyClass::get("NSString") {
                    let bytes = text.as_bytes();
                    let alloc: *mut objc2::runtime::AnyObject = msg_send![ns_str_cls, alloc];
                    let ns_str: *mut objc2::runtime::AnyObject = msg_send![
                        alloc,
                        initWithBytes: bytes.as_ptr(),
                        length: bytes.len(),
                        encoding: 4usize
                    ];
                    if !ns_str.is_null() {
                        let _: () = msg_send![pb, setString: ns_str];
                        let _: () = msg_send![ns_str, release];
                        return true;
                    }
                }
            }
        }
    }
    false
}
