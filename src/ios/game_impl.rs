use crate::core::game::Region;
use std::path::PathBuf;

pub struct IosGame;

pub fn get_package_name() -> String {
    "com.example.app".to_string()
}

pub fn get_region(_package_name: &str) -> Region {
    Region::Japan
}

pub fn get_data_dir(_package_name: &str) -> PathBuf {
    PathBuf::from("/var/mobile/Containers/Data/Application/YOUR_APP_UUID/Documents")
}