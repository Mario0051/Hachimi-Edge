use jni::objects::{GlobalRef, JObject, JValue};
use jni::{JNIEnv, JavaVM};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use semver::Version;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use blake3::Hasher as Blake3Hasher;

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize, Clone)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub download_url: String,
}

#[derive(Debug, Clone)]
pub enum DownloadState {
    Idle,
    Checking,
    UpdateAvailable(UpdateInfo),
    Downloading(String),
    Failed(String),
    Downloaded(PathBuf),
    Installing,
}

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_API_URL: Option<&str> = option_env!("HACHIMI_UPDATE_URL");
const APK_ASSET_NAME: &str = "umamusume.apk";
const HASH_ASSET_NAME: &str = "blake3.json";

static mut JAVA_VM: Option<JavaVM> = None;
static mut APP_CONTEXT: Option<GlobalRef> = None;
pub static DOWNLOAD_STATE: Lazy<Mutex<DownloadState>> = Lazy::new(|| Mutex::new(DownloadState::Idle));
pub static DOWNLOAD_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn init_updater(env: &JNIEnv, context_ref: GlobalRef) {
    unsafe {
        JAVA_VM = env.get_java_vm().ok();
        APP_CONTEXT = Some(context_ref);
    }
}

pub fn check_for_updates() {
    std::thread::spawn(check_for_updates_thread_impl);
}

pub fn trigger_download_and_install() {
    if let DownloadState::UpdateAvailable(info) = DOWNLOAD_STATE.lock().clone() {
        std::thread::spawn(move || {
            download_and_install_thread_impl(info.download_url);
        });
    }
}

fn get_current_apk_path() -> Option<PathBuf> {
    let (vm, context) = unsafe { (JAVA_VM.as_ref()?, APP_CONTEXT.as_ref()?) };
    let mut env = vm.attach_current_thread().ok()?;
    let context_obj = context.as_obj();

    let package_name_jobject = env
        .call_method(context_obj, "getPackageName", "()Ljava/lang/String;", &[])
        .ok()?
        .l()
        .ok()?;

    let package_manager_obj = env
        .call_method(
            context_obj,
            "getPackageManager",
            "()Landroid/content/pm/PackageManager;",
            &[],
        )
        .ok()?
        .l()
        .ok()?;

    let package_info_obj = env
        .call_method(
            package_manager_obj,
            "getPackageInfo",
            "(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;",
            &[JValue::Object(&package_name_jobject), JValue::Int(0)],
        )
        .ok()?
        .l()
        .ok()?;

    let app_info_obj = env
        .get_field(
            &package_info_obj,
            "applicationInfo",
            "Landroid/content/pm/ApplicationInfo;",
        )
        .ok()?
        .l()
        .ok()?;

    let source_dir_jobject = env
        .get_field(&app_info_obj, "sourceDir", "Ljava/lang/String;")
        .ok()?
        .l()
        .ok()?;

    let source_dir_path: String = env
        .get_string(&source_dir_jobject.into())
        .ok()?
        .into();

    Some(PathBuf::from(source_dir_path))
}

fn calculate_blake3(path: &PathBuf) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut hasher = Blake3Hasher::new();
    let mut buffer = [0; 8192];

    loop {
        let n = reader.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    let hash_bytes = hasher.finalize();
    Some(hash_bytes.to_hex().to_string())
}

fn check_for_updates_thread_impl() {
    let url = match GITHUB_API_URL {
        Some(url) => url,
        None => return,
    };

    *DOWNLOAD_STATE.lock() = DownloadState::Checking;

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("hachimi-updater-v{}", CURRENT_VERSION))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => {
            *DOWNLOAD_STATE.lock() = DownloadState::Failed(format!("Failed to build client: {}", e));
            return;
        }
    };

    let resp = match client.get(url).send() {
        Ok(resp) => resp,
        Err(e) => {
            *DOWNLOAD_STATE.lock() = DownloadState::Failed(format!("Failed to fetch: {}", e));
            return;
        }
    };

    let release: GitHubRelease = match resp.json() {
        Ok(release) => release,
        Err(e) => {
            *DOWNLOAD_STATE.lock() = DownloadState::Failed(format!("Failed to parse JSON: {}", e));
            return;
        }
    };

    let current_ver = Version::parse(CURRENT_VERSION).unwrap_or(Version::new(0, 0, 0));
    let release_ver_str = release.tag_name.trim_start_matches('v');
    let release_ver = Version::parse(release_ver_str).unwrap_or(Version::new(0, 0, 0));

    let apk_asset = match release.assets.iter().find(|a| a.name == APK_ASSET_NAME) {
        Some(asset) => asset.clone(),
        None => {
            *DOWNLOAD_STATE.lock() = DownloadState::Failed(format!(
                "Release {} found, but no '{}'",
                release.tag_name, APK_ASSET_NAME
            ));
            return;
        }
    };

    let update_info = UpdateInfo {
        version: release.tag_name.clone(),
        download_url: apk_asset.browser_download_url.clone(),
    };

    if release_ver > current_ver {
        *DOWNLOAD_STATE.lock() = DownloadState::UpdateAvailable(update_info);
    } else if release_ver == current_ver {

        let hash_asset = match release.assets.iter().find(|a| a.name == HASH_ASSET_NAME) {
            Some(asset) => asset,
            None => {
                *DOWNLOAD_STATE.lock() = DownloadState::Idle;
                return;
            }
        };

        let remote_hash = (|| -> Option<String> {
            let resp = client.get(&hash_asset.browser_download_url).send().ok()?;
            let hashes: HashMap<String, String> = resp.json().ok()?;
            hashes.get(APK_ASSET_NAME).cloned()
        })();

        let local_hash = get_current_apk_path().and_then(|path| calculate_blake3(&path));

        match (remote_hash, local_hash) {
            (Some(remote), Some(local)) if remote != local => {
                *DOWNLOAD_STATE.lock() = DownloadState::UpdateAvailable(update_info);
            }
            _ => {
                *DOWNLOAD_STATE.lock() = DownloadState::Idle;
            }
        }
    } else {
        *DOWNLOAD_STATE.lock() = DownloadState::Idle;
    }
}

fn download_and_install_thread_impl(url: String) {
    let (vm, context) = unsafe {
        if JAVA_VM.is_none() || APP_CONTEXT.is_none() {
            *DOWNLOAD_STATE.lock() = DownloadState::Failed("JNI VM not initialized".to_string());
            return;
        }
        (
            JAVA_VM.as_ref().unwrap(),
            APP_CONTEXT.as_ref().unwrap(),
        )
    };

    let mut env = vm.attach_current_thread().unwrap();
    let context_obj = context.as_obj();

    let cache_dir_obj = env
        .call_method(context_obj, "getCacheDir", "()Ljava/io/File;", &[])
        .unwrap()
        .l()
        .unwrap();

    let cache_dir_path_jstr = env
        .call_method(cache_dir_obj, "getAbsolutePath", "()Ljava/lang/String;", &[])
        .unwrap()
        .l()
        .unwrap();

    let cache_dir_path: String = env
        .get_string(&cache_dir_path_jstr.into())
        .unwrap()
        .into();

    let apk_path = PathBuf::from(cache_dir_path).join("umamusume-update.apk");

    {
        let mut state = DOWNLOAD_STATE.lock();
        *state = DownloadState::Downloading("Starting download...".to_string());
    }

    let mut resp = match reqwest::blocking::get(&url) {
        Ok(resp) => resp,
        Err(e) => {
            *DOWNLOAD_STATE.lock() = DownloadState::Failed(format!("Download failed: {}", e));
            return;
        }
    };

    let total_size = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut buffer = [0; 8192];
    let mut out = BufWriter::new(File::create(&apk_path).unwrap());

    while let Ok(len) = resp.read(&mut buffer) {
        if len == 0 {
            break;
        }
        out.write_all(&buffer[..len]).unwrap();
        downloaded += len as u64;

        let percent = if total_size > 0 {
            (downloaded as f32 / total_size as f32) * 100.0
        } else {
            0.0
        };
        *DOWNLOAD_STATE.lock() = DownloadState::Downloading(format!("Downloading... {:.1}%", percent));
    }
    drop(out);

    *DOWNLOAD_STATE.lock() = DownloadState::Downloaded(apk_path.clone());

    let apk_path_jstr = env.new_string(apk_path.to_str().unwrap()).unwrap();
    let file_obj = env
        .new_object(
            "java/io/File",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&apk_path_jstr.into())],
        )
        .unwrap();

    let package_name_jobject = env
        .call_method(context_obj, "getPackageName", "()Ljava/lang/String;", &[])
        .unwrap()
        .l()
        .unwrap();

    let package_name_str: String = env
        .get_string(&package_name_jobject.into())
        .unwrap()
        .into();

    let authority = env.new_string(format!("{}.provider", package_name_str)).unwrap();

    let context_class = env.get_object_class(context_obj).unwrap();

    let class_loader_obj = env
        .call_method(
            context_class,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .unwrap()
        .l()
        .unwrap();

    let class_name_jstr = env.new_string("androidx.core.content.FileProvider").unwrap();

    let file_provider_class_obj = env
        .call_method(
            &class_loader_obj,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name_jstr.into())],
        )
        .unwrap()
        .l()
        .unwrap();

    let file_provider_class: jni::objects::JClass<'_> = file_provider_class_obj.into();

    let uri_obj = env
        .call_static_method(
            file_provider_class,
            "getUriForFile",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/io/File;)Landroid/net/Uri;",
            &[
                JValue::Object(context_obj),
                JValue::Object(&authority.into()),
                JValue::Object(&file_obj),
            ],
        )
        .unwrap()
        .l()
        .unwrap();

    *DOWNLOAD_STATE.lock() = DownloadState::Installing;
    let intent_class = env.find_class("android/content/Intent").unwrap();

    let action_view = env.new_string("android.intent.action.VIEW").unwrap();

    let intent_obj = env
        .new_object(
            intent_class,
            "(Ljava/lang/String;)V",
            &[JValue::Object(&action_view.into())],
        )
        .unwrap();

    let mime_type = env
        .new_string("application/vnd.android.package-archive")
        .unwrap();

    env.call_method(
        &intent_obj,
        "setDataAndType",
        "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&uri_obj), JValue::Object(&mime_type.into())],
    )
    .unwrap();

    let flag_activity_new_task = 0x10000000;
    let flag_grant_read_uri = 0x00000001;

    env.call_method(
        &intent_obj,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[JValue::Int(flag_activity_new_task | flag_grant_read_uri)],
    )
    .unwrap();

    env.call_method(
        context_obj,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(&intent_obj)],
    )
    .unwrap();
}