use jni::objects::{GlobalRef, JObject, JValue};
use jni::{JNIEnv, JavaVM};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use semver::Version;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
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

fn check_for_updates_thread_impl() {
    let url = match GITHUB_API_URL {
        Some(url) => url,
        None => return,
    };

    *DOWNLOAD_STATE.lock() = DownloadState::Checking;

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("hachimi-updater-v{}", CURRENT_VERSION))
        .build();

    let resp = match client.and_then(|c| c.get(url).send()) {
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

    if release_ver > current_ver {
        if let Some(asset) = release.assets.iter().find(|a| a.name == APK_ASSET_NAME) {
            *DOWNLOAD_STATE.lock() = DownloadState::UpdateAvailable(UpdateInfo {
                version: release.tag_name,
                download_url: asset.browser_download_url.clone(),
            });
        } else {
            *DOWNLOAD_STATE.lock() =
                DownloadState::Failed(format!("Release {} found, but no '{}'", release.tag_name, APK_ASSET_NAME));
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