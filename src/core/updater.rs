use std::{sync::{Arc, Mutex}};

use rust_i18n::t;
use serde::Deserialize;

use crate::core::{gui::SimpleYesNoDialog, hachimi::{REPO_PATH, CODEBERG_API, GITHUB_API}, http, Error, Gui, Hachimi};

#[cfg(target_os = "android")]
use jni::{JavaVM, objects::{GlobalRef, JValue, JObject}};
#[cfg(target_os = "android")]
use std::{collections::HashMap, path::PathBuf};

#[derive(Default)]
pub struct Updater {
    update_check_mutex: Mutex<()>,
    new_update: arc_swap::ArcSwap<Option<ReleaseAsset>>,
    #[cfg(target_os = "android")]
    android_context: Mutex<Option<(JavaVM, GlobalRef)>>,
}

impl Updater {
    #[cfg(target_os = "android")]
    pub fn init_android(&self, vm: JavaVM, context: GlobalRef) {
        *self.android_context.lock().unwrap() = Some((vm, context));
    }

    pub fn check_for_updates(self: Arc<Self>, callback: fn(bool)) {
        std::thread::spawn(move || {
            match self.check_for_updates_internal() {
                Ok(v) => callback(v),
                Err(e) => error!("{}", e)
            }
        });
    }

    fn check_for_updates_internal(&self) -> Result<bool, Error> {
        // Prevent multiple update checks running at the same time
        let Ok(_guard) = self.update_check_mutex.try_lock() else {
            return Ok(false);
        };

        if let Some(mutex) = Gui::instance() {
            mutex.lock().unwrap().show_notification(&t!("notification.checking_for_updates"));
        }

        let latest = match http::get_json::<Release>(&format!("{}/{}/releases/latest", GITHUB_API, REPO_PATH)) {
            Ok(res) => res,
            Err(e) => {
                warn!("GitHub update check failed, trying Codeberg: {}", e);
                http::get_json::<Release>(&format!("{}/{}/releases/latest", CODEBERG_API, REPO_PATH))?
            }
        };

        if latest.is_different_version() {
            #[cfg(target_os = "windows")]
            {
                let installer_asset = latest.assets.iter().find(|asset| asset.name == "hachimi_installer.exe");
                let hash_asset = latest.assets.iter().find(|asset| asset.name == "blake3.json");

                if let (Some(installer), Some(h_json)) = (installer_asset, hash_asset) {
                    let hash_data = http::get_json::<HashMap<String, String>>(&h_json.browser_download_url)?;
                    let mut asset = installer.clone();
                    asset.expected_hash = hash_data.get("hachimi_installer.exe").cloned();
                    self.new_update.store(Arc::new(Some(asset)));

                    if let Some(mutex) = Gui::instance() {
                        mutex.lock().unwrap().show_window(Box::new(SimpleYesNoDialog::new(
                            &t!("update_prompt_dialog.title"),
                            &t!("update_prompt_dialog.content", version = latest.tag_name),
                            |ok| {
                                if !ok { return; }
                                Hachimi::instance().updater.clone().run();
                            }
                        )));
                    }
                    return Ok(true);
                }
            }
            #[cfg(target_os = "android")]
            {
                let apk_asset = latest.assets.iter().find(|asset| asset.name == "umamusume.apk");
                let hash_asset = latest.assets.iter().find(|asset| asset.name == "blake3.json");

                if let (Some(apk), Some(h_json)) = (apk_asset, hash_asset) {
                     let hash_data = http::get_json::<HashMap<String, String>>(&h_json.browser_download_url)?;
                     let remote_hash = hash_data.get("umamusume.apk");

                     if let Some(remote) = remote_hash {
                        if let Some(local) = self.get_current_apk_hash() {
                            if remote == &local {
                                return Ok(false);
                            }
                        }
                     }

                     let mut asset = apk.clone();
                     asset.expected_hash = remote_hash.cloned();
                     self.new_update.store(Arc::new(Some(asset)));

                     if let Some(mutex) = Gui::instance() {
                        mutex.lock().unwrap().show_window(Box::new(SimpleYesNoDialog::new(
                            &t!("update_prompt_dialog.title"),
                            &t!("update_prompt_dialog.content", version = latest.tag_name),
                            |ok| {
                                if !ok { return; }
                                Hachimi::instance().updater.clone().run();
                            }
                        )));
                    }
                    return Ok(true);
                }
            }
        } else if let Some(mutex) = Gui::instance() {
            mutex.lock().unwrap().show_notification(&t!("notification.no_updates"));
        }

        Ok(false)
    }

    #[cfg(target_os = "android")]
    fn get_current_apk_hash(&self) -> Option<String> {
        let guard = self.android_context.lock().unwrap();
        let (vm, context) = guard.as_ref()?;
        let mut env = vm.attach_current_thread().ok()?;
        let context_obj = context.as_obj();

        let package_name = env.call_method(context_obj, "getPackageName", "()Ljava/lang/String;", &[]).ok()?.l().ok()?;

        let package_manager = env.call_method(context_obj, "getPackageManager", "()Landroid/content/pm/PackageManager;", &[]).ok()?.l().ok()?;

        let package_info = env.call_method(
            package_manager,
            "getPackageInfo",
            "(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;",
            &[JValue::Object(&package_name), JValue::Int(0)]
        ).ok()?.l().ok()?;

        let app_info = env.get_field(&package_info, "applicationInfo", "Landroid/content/pm/ApplicationInfo;").ok()?.l().ok()?;

        let source_dir_jstr = env.get_field(&app_info, "sourceDir", "Ljava/lang/String;").ok()?.l().ok()?;
        let source_dir: String = env.get_string(&source_dir_jstr.into()).ok()?.into();

        use std::io::Read;
        let file = std::fs::File::open(source_dir).ok()?;
        let mut reader = std::io::BufReader::new(file);
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0; 8192];
        loop {
            let n = reader.read(&mut buffer).ok()?;
            if n == 0 { break; }
            hasher.update(&buffer[..n]);
        }
        Some(hasher.finalize().to_hex().to_string())
    }

    pub fn run(self: Arc<Self>) {
        std::thread::spawn(move || {
            let dialog_show = Arc::new(std::sync::atomic::AtomicBool::new(true));
            if let Some(mutex) = Gui::instance() {
                mutex.lock().unwrap().show_window(Box::new(crate::core::gui::PersistentMessageWindow::new(
                    &t!("updating_dialog.title"),
                    &t!("updating_dialog.content"),
                    dialog_show.clone()
                )));
            }

            if let Err(e) = self.clone().run_internal() {
                error!("{}", e);
                if let Some(mutex) = Gui::instance() {
                    mutex.lock().unwrap().show_notification(&t!("notification.update_failed", reason = e.to_string()));
                }
            }

            dialog_show.store(false, std::sync::atomic::Ordering::Relaxed)
        });
    }

    fn run_internal(self: Arc<Self>) -> Result<(), Error> {
        let Some(ref asset) = **self.new_update.load() else {
            return Ok(());
        };
        self.new_update.store(Arc::new(None));

        #[cfg(target_os = "windows")]
        {
            use crate::windows::{main::DLL_HMODULE, utils};
            use windows::{
                core::{HSTRING, PCWSTR},
                Win32::{
                    Foundation::{MAX_PATH, WPARAM, LPARAM}, System::LibraryLoader::GetModuleFileNameW,
                    UI::{Shell::ShellExecuteW, WindowsAndMessaging::{PostMessageW, SW_NORMAL, WM_CLOSE}}
                }
            };
            use std::{fs::File, io::Read};

            // Download the installer
            let installer_path = utils::get_tmp_installer_path();

            let res = ureq::get(&asset.browser_download_url).call()?;
            let mut reader = res.into_body().into_reader();
            std::io::copy(&mut reader, &mut File::create(&installer_path)?)?;

            // Verify the installer
            if let Some(expected_hash) = &asset.expected_hash {
                let mut file = File::open(&installer_path)?;
                let mut hasher = blake3::Hasher::new();
                let mut buffer = [0u8; 8192];

                while let Ok(n) = file.read(&mut buffer) {
                    if n == 0 { break; }
                    hasher.update(&buffer[..n]);
                }

                if hasher.finalize().to_hex().as_str() != expected_hash {
                    let _ = std::fs::remove_file(&installer_path);
                    return Err(Error::FileHashMismatch(installer_path.to_string_lossy().into()));
                }
            }

            // Launch the installer
            let mut slice = [0u16; MAX_PATH as usize];
            let length = unsafe { GetModuleFileNameW(Some(DLL_HMODULE), &mut slice) } as usize;
            let hachimi_path_str = unsafe { widestring::Utf16Str::from_slice_unchecked(&slice[..length]) };
            let game_dir = utils::get_game_dir();
            unsafe {
                ShellExecuteW(
                    None,
                    None,
                    &HSTRING::from(installer_path.into_os_string()),
                    &HSTRING::from(format!(
                        "install --install-dir \"{}\" --target \"{}\" --sleep 1000 --prompt-for-game-exit --launch-game -- {}",
                        game_dir.display(), hachimi_path_str, std::env::args().skip(1).collect::<Vec<String>>().join(" ")
                    )),
                    PCWSTR::from_raw(slice.as_ptr()),
                    SW_NORMAL
                );

                // Close the game
                _ = PostMessageW(None, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }

        #[cfg(target_os = "android")]
        {
            use std::{fs::File, io::Read, io::Write};

            let guard = self.android_context.lock().unwrap();
            let (vm, context) = guard.as_ref().ok_or(Error::Msg("JNI Context not initialized".into()))?;
            let mut env = vm.attach_current_thread().map_err(|_| Error::Msg("Failed to attach thread".into()))?;
            let context_obj = context.as_obj();

            let cache_dir_obj = env.call_method(context_obj, "getCacheDir", "()Ljava/io/File;", &[]).unwrap().l().unwrap();
            let cache_dir_path_jstr = env.call_method(cache_dir_obj, "getAbsolutePath", "()Ljava/lang/String;", &[]).unwrap().l().unwrap();
            let cache_dir_path: String = env.get_string(&cache_dir_path_jstr.into()).unwrap().into();

            let apk_path = PathBuf::from(cache_dir_path).join("umamusume-update.apk");

            let res = ureq::get(&asset.browser_download_url).call()?;
            let total_size = res.header("Content-Length").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);

            let mut reader = res.into_body().into_reader();
            let mut out = std::io::BufWriter::new(File::create(&apk_path)?);
            let mut buffer = [0; 8192];
            let mut _downloaded: u64 = 0;

            loop {
                let len = reader.read(&mut buffer)?;
                if len == 0 { break; }
                out.write_all(&buffer[..len])?;
                _downloaded += len as u64;
            }
            drop(out);

             if let Some(expected_hash) = &asset.expected_hash {
                let file = File::open(&apk_path)?;
                let mut reader = std::io::BufReader::new(file);
                let mut hasher = blake3::Hasher::new();
                let mut buffer = [0u8; 8192];
                loop {
                    let n = reader.read(&mut buffer)?;
                    if n == 0 { break; }
                    hasher.update(&buffer[..n]);
                }
                if hasher.finalize().to_hex().as_str() != expected_hash {
                     let _ = std::fs::remove_file(&apk_path);
                    return Err(Error::FileHashMismatch(apk_path.to_string_lossy().into()));
                }
            }

            let apk_path_jstr = env.new_string(apk_path.to_str().unwrap()).unwrap();
            let file_obj = env.new_object("java/io/File", "(Ljava/lang/String;)V", &[JValue::Object(&apk_path_jstr.into())]).unwrap();

            let package_name_jobj = env.call_method(context_obj, "getPackageName", "()Ljava/lang/String;", &[]).unwrap().l().unwrap();
            let package_name: String = env.get_string(&package_name_jobj.into()).unwrap().into();
            let authority = env.new_string(format!("{}.provider", package_name)).unwrap();

            let context_class = env.get_object_class(context_obj).unwrap();
            let class_loader = env.call_method(context_class, "getClassLoader", "()Ljava/lang/ClassLoader;", &[]).unwrap().l().unwrap();
            let file_provider_class_name = env.new_string("androidx.core.content.FileProvider").unwrap();
            let file_provider_class = env.call_method(class_loader, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;", &[JValue::Object(&file_provider_class_name.into())]).unwrap().l().unwrap();

            let uri_obj = env.call_static_method(
                file_provider_class.into(),
                "getUriForFile",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/io/File;)Landroid/net/Uri;",
                &[JValue::Object(context_obj), JValue::Object(&authority.into()), JValue::Object(&file_obj)]
            ).unwrap().l().unwrap();

            let intent_class = env.find_class("android/content/Intent").unwrap();
            let action_view = env.new_string("android.intent.action.VIEW").unwrap();
            let intent_obj = env.new_object(intent_class, "(Ljava/lang/String;)V", &[JValue::Object(&action_view.into())]).unwrap();
            let mime_type = env.new_string("application/vnd.android.package-archive").unwrap();

            env.call_method(&intent_obj, "setDataAndType", "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;", &[JValue::Object(&uri_obj), JValue::Object(&mime_type.into())]).unwrap();

            let flags = 0x10000000 | 0x00000001;
            env.call_method(&intent_obj, "addFlags", "(I)Landroid/content/Intent;", &[JValue::Int(flags)]).unwrap();

            env.call_method(context_obj, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&intent_obj)]).unwrap();
        }

        Ok(())
    }
}

#[derive(Deserialize)]
pub struct Release {
    // STUB
    tag_name: String,
    assets: Vec<ReleaseAsset>
}

impl Release {
    pub fn is_different_version(&self) -> bool {
        self.tag_name != format!("v{}", env!("CARGO_PKG_VERSION"))
    }
}

#[derive(Deserialize, Clone)]
pub struct ReleaseAsset {
    // STUB
    name: String,
    browser_download_url: String,
    #[serde(skip)]
    pub expected_hash: Option<String>
}