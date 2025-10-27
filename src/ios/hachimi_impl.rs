use crate::core::hachimi::Hachimi;

pub struct IosHachimi;

pub fn is_il2cpp_lib(filename: &str) -> bool {
    filename.contains("UnityFramework")
}

pub fn is_criware_lib(_filename: &str) -> bool {
    false
}

pub fn on_hooking_finished(_hachimi: &Hachimi) {
    info!("iOS platform-specific post-hooking logic can run here.");
}