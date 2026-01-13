use std::{fs::File, io::Read, sync::{atomic::{self, AtomicBool}, Arc, Mutex}};

use arc_swap::ArcSwap;
use rust_i18n::t;
use serde::Deserialize;
use widestring::Utf16Str;
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::{MAX_PATH, WPARAM, LPARAM}, System::LibraryLoader::GetModuleFileNameW,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::{PostMessageW, SW_NORMAL, WM_CLOSE}}
    }
};

use crate::core::{gui::{PersistentMessageWindow, SimpleYesNoDialog}, http, Error, Gui, Hachimi};

use super::{main::DLL_HMODULE, utils};

const REPO_PATH: &str = "kairusds/Hachimi-Edge";
const GITHUB_API: &str = "https://api.github.com/repos";
const CODEBERG_API: &str = "https://codeberg.org/api/v1/repos";

#[derive(Default)]
pub struct Updater {
    update_check_mutex: Mutex<()>,
    new_update: ArcSwap<Option<ReleaseAsset>>
}

impl Updater {
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
            let installer_asset = latest.assets.iter().find(|asset| asset.name == "hachimi_installer.exe");
            let hash_asset = latest.assets.iter().find(|asset| asset.name == "blake3.json");

            if let (Some(installer), Some(h_json)) = (installer_asset, hash_asset) {
                let hash_data = http::get_json::<Blake3Hashes>(&h_json.browser_download_url)?;
                let mut asset = installer.clone();
                asset.expected_hash = Some(hash_data.installer_exe);
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
        } else if let Some(mutex) = Gui::instance() {
            mutex.lock().unwrap().show_notification(&t!("notification.no_updates"));
        }

        Ok(false)
    }

    pub fn run(self: Arc<Self>) {
        std::thread::spawn(move || {
            let dialog_show = Arc::new(AtomicBool::new(true));
            if let Some(mutex) = Gui::instance() {
                mutex.lock().unwrap().show_window(Box::new(PersistentMessageWindow::new(
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

            dialog_show.store(false, atomic::Ordering::Relaxed)
        });
    }

    fn run_internal(self: Arc<Self>) -> Result<(), Error> {
        let Some(ref asset) = **self.new_update.load() else {
            return Ok(());
        };
        self.new_update.store(Arc::new(None));

        // Download the installer
        let installer_path = utils::get_tmp_installer_path();

        let res = ureq::get(&asset.browser_download_url).call()?;
        std::io::copy(&mut res.into_reader(), &mut File::create(&installer_path)?)?;

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
        let hachimi_path_str = unsafe { Utf16Str::from_slice_unchecked(&slice[..length]) };
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

#[derive(Deserialize)]
struct Blake3Hashes {
    #[serde(rename = "hachimi_installer.exe")]
    installer_exe: String
}
