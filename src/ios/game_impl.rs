use crate::core::game::Region;
use std::path::PathBuf;

use objc2_foundation::{NSBundle, NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask};

pub struct IosGame;

pub fn get_package_name() -> String {
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().expect("Could not get bundle identifier").to_string()
}

pub fn get_region(package_name: &str) -> Region {
    match package_name {
        "jp.co.cygames.umamusume" => Region::Japan,
        "com.kakaogames.umamusume" => Region::Korea,
        "com.bilibili.umamusu" => Region::China,
        "com.komoe.umamusume.tc" => Region::Taiwan,
        _ => Region::Unknown,
    }
}

pub fn get_data_dir(_package_name: &str) -> PathBuf {
    let file_manager = NSFileManager::defaultManager();

    let urls = file_manager.URLsForDirectory_inDomains(
        NSSearchPathDirectory::DocumentDirectory,
        NSSearchPathDomainMask::UserDomainMask,
    );

    let documents_url = urls.firstObject().expect("Could not find Documents directory");

    PathBuf::from(documents_url.path().expect("URL had no path").to_string())
}