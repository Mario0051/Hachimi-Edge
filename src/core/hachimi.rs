use std::{fs, path::{Path, PathBuf}, process, sync::{atomic::{self, AtomicBool, AtomicI32}, Arc, Mutex}};
use arc_swap::ArcSwap;
use fnv::{FnvHashMap, FnvHashSet};
use once_cell::sync::OnceCell;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use textwrap::wrap_algorithms::Penalties;

use crate::{core::{plugin_api::Plugin, updater}, gui_impl, hachimi_impl, il2cpp::{self, hook::umamusume::{CySpringController::SpringUpdateMode, GameSystem}, sql::{CharacterData, SkillInfo}}};

use super::{game::{Game, Region}, ipc, plurals, template, template_filters, tl_repo, utils, Error, Interceptor};

pub const REPO_PATH: &str = "kairusds/Hachimi-Edge";
pub const GITHUB_API: &str = "https://api.github.com/repos";
pub const CODEBERG_API: &str = "https://codeberg.org/api/v1/repos";
pub const WEBSITE_URL: &str = "https://hachimi.noccu.art";
pub const UMAPATCHER_PACKAGE_NAME: &str = "com.leadrdrk.umapatcher.edge";
pub const UMAPATCHER_INSTALL_URL: &str = "https://github.com/kairusds/UmaPatcher-Edge/releases/latest";

pub static CONFIG_LOAD_ERROR: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
struct SurgicalRepair {
    range: std::ops::Range<usize>,
    replacement: String,
}

struct LenientParser<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    input: &'a str,
    input_len: usize,
    spans: FnvHashMap<String, std::ops::Range<usize>>,
    syntax_repairs: Vec<SurgicalRepair>,
    current_path: Vec<String>,
}

impl<'a> LenientParser<'a> {
    fn new(input: &'a str) -> Self {
        let mut chars = input.char_indices().peekable();
        if let Some((_, '\u{feff}')) = chars.peek() { chars.next(); }
        Self {
            chars,
            input,
            input_len: input.len(),
            spans: FnvHashMap::default(),
            syntax_repairs: Vec::new(),
            current_path: Vec::new(),
        }
    }

    fn current_pos(&mut self) -> usize {
        self.chars.peek().map(|(i, _)| *i).unwrap_or(self.input_len)
    }

    fn consume_ignorable(&mut self) {
        loop {
            match self.chars.peek() {
                Some(&(i, c)) if c.is_whitespace() => {
                    self.chars.next();
                },
                Some(&(start_idx, '/')) => {
                    let mut lookahead = self.chars.clone();
                    lookahead.next();
                    match lookahead.peek() {
                        Some(&(_, '/')) => {
                            self.chars.next(); self.chars.next();
                            while let Some((_, n)) = self.chars.next() {
                                if n == '\n' || n == '\r' { break; }
                            }
                            let end_idx = self.current_pos();
                            self.syntax_repairs.push(SurgicalRepair {
                                range: start_idx..end_idx,
                                replacement: "\n".to_string()
                            });
                        },
                        Some(&(_, '*')) => {
                            self.chars.next(); self.chars.next();
                            while let Some((_, n)) = self.chars.next() {
                                if n == '*' {
                                    if let Some(&(_, '/')) = self.chars.peek() { self.chars.next(); break; }
                                }
                            }
                            let end_idx = self.current_pos();
                            self.syntax_repairs.push(SurgicalRepair {
                                range: start_idx..end_idx,
                                replacement: " ".to_string()
                            });
                        },
                        _ => break,
                    }
                },
                _ => break,
            }
        }
    }

    fn record_span<F>(&mut self, parse_fn: F) -> serde_json::Value
    where F: FnOnce(&mut Self) -> serde_json::Value
    {
        self.consume_ignorable();
        let start = self.current_pos();
        let val = parse_fn(self);
        let end = self.current_pos();

        if !self.current_path.is_empty() {
            let key = self.current_path.join(".");
            self.spans.insert(key, start..end);
        }
        val
    }

    fn consume_root_garbage(&mut self) {
        loop {
            self.consume_ignorable();
            match self.chars.peek() {
                Some(&(idx, '}')) | Some(&(idx, ']')) => {
                    self.syntax_repairs.push(SurgicalRepair {
                        range: idx..idx+1,
                        replacement: "".to_string()
                    });
                    self.chars.next();
                },
                _ => break
            }
        }
    }

    fn consume_trailing_garbage(&mut self) {
        self.consume_ignorable();

        if self.chars.peek().is_some() {
            let start = self.current_pos();
            while self.chars.next().is_some() {}
            let end = self.current_pos();

            self.syntax_repairs.push(SurgicalRepair {
                range: start..end,
                replacement: "".to_string()
            });
        }
    }

    fn parse_root(&mut self) -> serde_json::Value {
        self.consume_root_garbage();

        let val = if let Some(&(_, '{')) = self.chars.peek() {
            self.parse_object()
        } else {
            self.parse_object_body()
        };

        self.consume_trailing_garbage();

        val
    }

    fn parse_object(&mut self) -> serde_json::Value {
        self.chars.next();
        self.parse_object_body()
    }

    fn parse_object_body(&mut self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        loop {
            self.consume_ignorable();
            if let Some(&(_, '}')) = self.chars.peek() { self.chars.next(); break; }
            if self.chars.peek().is_none() { break; }

            let key = self.parse_key_auto_fix();

            self.consume_ignorable();
            if let Some(&(_, c)) = self.chars.peek() { if c == ':' || c == '=' { self.chars.next(); } }

            self.current_path.push(key.clone());
            let val = self.record_span(|p| p.parse_value());
            self.current_path.pop();

            map.insert(key, val);

            let post_val_pos = self.current_pos();
            self.consume_ignorable();

            if let Some(&(comma_idx, ',')) = self.chars.peek() {
                self.chars.next();

                self.consume_ignorable();
                if let Some(&(_, '}')) = self.chars.peek() {
                    self.syntax_repairs.push(SurgicalRepair {
                        range: comma_idx..comma_idx+1,
                        replacement: "".to_string()
                    });
                    self.chars.next();
                    break;
                }
            } else {
                if let Some(&(_, '}')) = self.chars.peek() {
                    self.chars.next();
                    break;
                } else {
                    self.syntax_repairs.push(SurgicalRepair {
                        range: post_val_pos..post_val_pos,
                        replacement: ",".to_string()
                    });
                }
            }
        }
        serde_json::Value::Object(map)
    }

    fn parse_array(&mut self) -> serde_json::Value {
        let mut vec = Vec::new();
        self.chars.next();
        loop {
            self.consume_ignorable();
            if let Some(&(_, ']')) = self.chars.peek() { self.chars.next(); break; }
            if self.chars.peek().is_none() { break; }

            vec.push(self.record_span(|p| p.parse_value()));

            let post_val_pos = self.current_pos();
            self.consume_ignorable();
            if let Some(&(comma_idx, ',')) = self.chars.peek() {
                self.chars.next();

                self.consume_ignorable();
                if let Some(&(_, ']')) = self.chars.peek() {
                    self.syntax_repairs.push(SurgicalRepair {
                        range: comma_idx..comma_idx+1,
                        replacement: "".to_string()
                    });
                    self.chars.next();
                    break;
                }
            } else {
                if let Some(&(_, ']')) = self.chars.peek() {
                    self.chars.next();
                    break;
                } else {
                    self.syntax_repairs.push(SurgicalRepair {
                        range: post_val_pos..post_val_pos,
                        replacement: ",".to_string()
                    });
                }
            }
        }
        serde_json::Value::Array(vec)
    }

    fn parse_key_auto_fix(&mut self) -> String {
        self.consume_ignorable();

        let start_pos = self.current_pos();
        if let Some(&(_, c)) = self.chars.peek() {
            if "\"'“‘".contains(c) {
                if let serde_json::Value::String(s) = self.parse_string() { return s; }
            }
        }

        let mut s = String::new();
        while let Some(&(_, c)) = self.chars.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' { s.push(self.chars.next().unwrap().1); } else { break; }
        }
        let end_pos = self.current_pos();

        if !s.is_empty() {
            self.syntax_repairs.push(SurgicalRepair {
                range: start_pos..end_pos,
                replacement: format!("\"{}\"", s)
            });
        }

        s
    }

    fn parse_string(&mut self) -> serde_json::Value {
        let (start_idx, start_char) = self.chars.next().unwrap();

        let is_smart_quote = "“‘".contains(start_char);
        let end_char = match start_char { '“' => '”', '‘' => '’', c => c };

        let mut s = String::new();
        while let Some((idx, c)) = self.chars.next() {
            if c == end_char {
                if is_smart_quote {
                    self.syntax_repairs.push(SurgicalRepair { range: start_idx..start_idx+start_char.len_utf8(), replacement: "\"".into() });
                    self.syntax_repairs.push(SurgicalRepair { range: idx..idx+end_char.len_utf8(), replacement: "\"".into() });
                }
                break;
            }
            if c == '\\' {
                if let Some((_, n)) = self.chars.next() {
                    match n {
                        'n' => s.push('\n'), 'r' => s.push('\r'), 't' => s.push('\t'),
                        '"' => s.push('"'), '\\' => s.push('\\'),
                        'u' => {
                            let mut hex = String::new();
                            for _ in 0..4 { if let Some((_, h)) = self.chars.next() { hex.push(h); } }
                            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                if let Some(ch) = std::char::from_u32(code) { s.push(ch); }
                            }
                        },
                        _ => { s.push('\\'); s.push(n); }
                    }
                }
            } else { s.push(c); }
        }
        serde_json::Value::String(s)
    }

    fn parse_value(&mut self) -> serde_json::Value {
        self.consume_ignorable();
        match self.chars.peek() {
            Some(&(_, '{')) => self.parse_object(),
            Some(&(_, '[')) => self.parse_array(),
            Some(&(_, '"')) | Some(&(_, '\'')) | Some(&(_, '“')) | Some(&(_, '”')) | Some(&(_, '‘')) | Some(&(_, '’')) => self.parse_string(),
            Some(&(_, 't')) | Some(&(_, 'T')) => { self.consume_word(); serde_json::Value::Bool(true) },
            Some(&(_, 'f')) | Some(&(_, 'F')) => { self.consume_word(); serde_json::Value::Bool(false) },
            Some(&(_, 'n')) | Some(&(_, 'N')) => { self.consume_word(); serde_json::Value::Null },
            Some(&(_, c)) if c.is_ascii_digit() || c == '-' || c == '.' => self.parse_number(),
            Some(_) => { self.chars.next(); self.parse_value() },
            None => serde_json::Value::Null
        }
    }

    fn consume_word(&mut self) { while let Some(&(_, c)) = self.chars.peek() { if c.is_alphabetic() { self.chars.next(); } else { break; } } }

    fn parse_number(&mut self) -> serde_json::Value {
        let mut s = String::new();
        while let Some(&(_, c)) = self.chars.peek() {
            if c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == 'E' { s.push(self.chars.next().unwrap().1); } else { break; }
        }
        if let Ok(n) = s.parse::<i64>() { serde_json::json!(n) } else if let Ok(n) = s.parse::<f64>() { serde_json::json!(n) } else { serde_json::Value::Null }
    }
}

fn load_and_repair_json<T>(path: &Path) -> Result<T, Error>
where T: DeserializeOwned + Serialize + Default + Clone
{
    if fs::metadata(path).is_err() {
        return Ok(T::default());
    }

    let json_content = fs::read_to_string(path)?;
    let mut parser = LenientParser::new(&json_content);
    let raw_json_val = parser.parse_root();

    let default_config = T::default();
    let mut final_config_val = serde_json::to_value(&default_config).map_err(Error::JsonParseError)?;
    let default_root = final_config_val.clone();

    let mut repairs: Vec<SurgicalRepair> = parser.syntax_repairs;

    if let Some(user_map) = raw_json_val.as_object() {
        if let Some(final_map) = final_config_val.as_object_mut() {
            for (k, v) in user_map {
                let backup = final_map.get(k).cloned();
                final_map.insert(k.clone(), v.clone());

                if serde_json::from_value::<T>(serde_json::Value::Object(final_map.clone())).is_err() {
                    warn!("Invalid config field '{}' detected.", k);

                    let mut merged_val = default_root.get(k).cloned().unwrap_or(serde_json::Value::Null);
                    let mut repair_target_path = k.clone();
                    let mut replacement_val = merged_val.clone();

                    if v.is_object() && merged_val.is_object() {
                        let user_obj = v.as_object().unwrap();
                         for (sub_k, sub_v) in user_obj {
                             let sub_backup = {
                                 let merged_obj = merged_val.as_object_mut().unwrap();
                                 let backup = merged_obj.get(sub_k).cloned();
                                 merged_obj.insert(sub_k.clone(), sub_v.clone());
                                 backup
                             };

                             final_map.insert(k.clone(), merged_val.clone());
                             if serde_json::from_value::<T>(serde_json::Value::Object(final_map.clone())).is_err() {
                                 warn!("Identified culprit: '{}.{}'. Reverting to default.", k, sub_k);
                                 let merged_obj = merged_val.as_object_mut().unwrap();
                                 if let Some(old) = sub_backup {
                                     merged_obj.insert(sub_k.clone(), old.clone());
                                     replacement_val = old;
                                 } else {
                                     merged_obj.remove(sub_k);
                                 }
                                 repair_target_path = format!("{}.{}", k, sub_k);
                             }
                         }
                    } else {
                        merged_val = default_root.get(k).cloned().unwrap_or(serde_json::Value::Null);
                        replacement_val = merged_val.clone();
                    }

                    final_map.insert(k.clone(), merged_val);

                    if let Some(span) = parser.spans.get(&repair_target_path) {
                         let replacement_str = serde_json::to_string(&replacement_val).unwrap_or_default();

                         repairs.retain(|r| {
                             let is_inside = r.range.start >= span.start && r.range.end <= span.end;
                             !is_inside
                         });

                         repairs.push(SurgicalRepair { range: span.clone(), replacement: replacement_str });
                    }
                }
            }
        }
    }

    let final_config: T = serde_json::from_value(final_config_val).map_err(Error::JsonParseError)?;

    if !repairs.is_empty() {
        info!("Applying {} surgical repairs to config file...", repairs.len());
        repairs.sort_by(|a, b| b.range.start.cmp(&a.range.start));

        let mut fixed_content = json_content.clone();
        for repair in repairs {
            if repair.range.end <= fixed_content.len() {
                fixed_content.replace_range(repair.range, &repair.replacement);
            }
        }
        let _ = fs::write(path, fixed_content);
    }

    Ok(final_config)
}

pub struct Hachimi {
    // Hooking stuff
    pub interceptor: Interceptor,
    pub hooking_finished: AtomicBool,
    pub plugins: Mutex<Vec<Plugin>>,

    // Localized data
    pub localized_data: ArcSwap<LocalizedData>,
    pub tl_updater: Arc<tl_repo::Updater>,

    // Character data
    pub chara_data: ArcSwap<CharacterData>,
    // Untranslated skill info
    pub skill_info: ArcSwap<SkillInfo>,

    // Shared properties
    pub game: Game,
    pub config: ArcSwap<Config>,
    pub template_parser: template::Parser,

    /// -1 = default
    pub target_fps: AtomicI32,

    #[cfg(target_os = "windows")]
    pub vsync_count: AtomicI32,

    #[cfg(target_os = "windows")]
    pub window_always_on_top: AtomicBool,

    #[cfg(target_os = "windows")]
    pub discord_rpc: AtomicBool,

    pub updater: Arc<updater::Updater>
}

static INSTANCE: OnceCell<Arc<Hachimi>> = OnceCell::new();

impl Hachimi {
    pub fn init() -> bool {
        if INSTANCE.get().is_some() {
            warn!("Hachimi should be initialized only once");
            return true;
        }

        let instance = match Self::new() {
            Ok(v) => v,
            Err(e) => {
                super::log::init(false, false); // early init to log error
                error!("Init failed: {}", e);
                return false;
            }
        };

        let config = instance.config.load();
        if config.disable_gui_once {
            let mut config = config.as_ref().clone();
            config.disable_gui_once = false;
            _ = instance.save_config(&config);

            config.disable_gui = true;
            instance.config.store(Arc::new(config));
        }

        super::log::init(config.debug_mode, config.enable_file_logging);

        info!("Hachimi {}", env!("HACHIMI_DISPLAY_VERSION"));
        info!("Game region: {}", instance.game.region);
        instance.load_localized_data();

        INSTANCE.set(Arc::new(instance)).is_ok()
    }

    pub fn instance() -> Arc<Hachimi> {
        INSTANCE.get().unwrap_or_else(|| {
            error!("FATAL: Attempted to get Hachimi instance before initialization");
            process::exit(1);
        }).clone()
    }

    pub fn is_initialized() -> bool {
        INSTANCE.get().is_some()
    }

    fn new() -> Result<Hachimi, Error> {
        let game = Game::init();
        let config = Self::load_config(&game.data_dir, &game.region)?;

        config.language.set_locale();

        Ok(Hachimi {
            interceptor: Interceptor::default(),
            hooking_finished: AtomicBool::new(false),
            plugins: Mutex::default(),

            // Don't load localized data initially since it might fail, logging the error is not possible here
            localized_data: ArcSwap::default(),
            tl_updater: Arc::default(),

            // Same with these
            chara_data: ArcSwap::default(),
            skill_info: ArcSwap::default(),

            game,
            template_parser: template::Parser::new(&template_filters::LIST),

            target_fps: AtomicI32::new(config.target_fps.unwrap_or(-1)),

            #[cfg(target_os = "windows")]
            vsync_count: AtomicI32::new(config.windows.vsync_count),

            #[cfg(target_os = "windows")]
            window_always_on_top: AtomicBool::new(config.windows.window_always_on_top),

            #[cfg(target_os = "windows")]
            discord_rpc: AtomicBool::new(config.windows.discord_rpc),

            updater: Arc::default(),

            config: ArcSwap::new(Arc::new(config))
        })
    }

    fn load_config(data_dir: &Path, _region: &Region) -> Result<Config, Error> {
        let config_path = data_dir.join("config.json");

        match load_and_repair_json(&config_path) {
            Ok(config) => Ok(config),
            Err(e) => {
                eprintln!("Failed to parse config: {}", e);
                CONFIG_LOAD_ERROR.store(true, std::sync::atomic::Ordering::Release);
                Ok(Config::default())
            }
        }
    }

    pub fn reload_config(&self) {
        let new_config = match Self::load_config(&self.game.data_dir, &self.game.region) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to reload config: {}", e);
                return;
            }
        };

        new_config.language.set_locale();
        self.config.store(Arc::new(new_config));
    }

    pub fn save_config(&self, config: &Config) -> Result<(), Error> {
        fs::create_dir_all(&self.game.data_dir)?;
        let config_path = self.get_data_path("config.json");
        utils::write_json_file(config, &config_path)?;

        Ok(())
    }

    pub fn save_and_reload_config(&self, config: Config) -> Result<(), Error> {
        self.save_config(&config)?;

        config.language.set_locale();
        self.config.store(Arc::new(config));
        Ok(())
    }

    pub fn load_localized_data(&self) {
        if self.tl_updater.progress().is_some() {
            warn!("Update in progress, not loading localized data");
            return;
        }
        let new_data = match LocalizedData::new(&self.config.load(), &self.game.data_dir) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to load localized data: {}", e);
                return;
            }
        };
        self.localized_data.store(Arc::new(new_data));
    }

    pub fn init_character_data(&self) {
        if self.chara_data.load().chara_ids.is_empty() {
            let data = CharacterData::load_from_db();
            self.chara_data.store(Arc::new(data));
            info!("Character database loaded successfully.");
        }
    }

    pub fn init_skill_info(&self) {
        if self.skill_info.load().skill_names.is_empty() {
            let data = SkillInfo::load_from_db();
            self.skill_info.store(Arc::new(data));
            info!("Skill info loaded successfully.");
        }
    }

    pub fn on_dlopen(&self, filename: &str, handle: usize) -> bool {
        // Prevent double initialization
        if self.hooking_finished.load(atomic::Ordering::Relaxed) { return false; }

        if hachimi_impl::is_il2cpp_lib(filename) {
            info!("Got il2cpp handle");
            il2cpp::symbols::set_handle(handle);
            false
        }
        else if hachimi_impl::is_criware_lib(filename) {
            self.on_hooking_finished();
            true
        }
        else {
            false
        }
    }

    pub fn on_hooking_finished(&self) {
        self.hooking_finished.store(true, atomic::Ordering::Relaxed);

        info!("GameAssembly finished loading");
        il2cpp::symbols::init();
        il2cpp::hook::init();

        // By the time it finished hooking the game will have already finished initializing
        GameSystem::on_game_initialized();

        let config = self.config.load();
        if !config.disable_gui {
            gui_impl::init();
        }

        if config.enable_ipc {
            ipc::start_http(config.ipc_listen_all);
        }

        hachimi_impl::on_hooking_finished(self);

        for plugin in self.plugins.lock().unwrap().iter() {
            info!("Initializing plugin: {}", plugin.name);
            let res = plugin.init();
            if !res.is_ok() {
                info!("Plugin init failed");
            }
        }
    }

    pub fn get_data_path<P: AsRef<Path>>(&self, rel_path: P) -> PathBuf {
        self.game.data_dir.join(rel_path)
    }

    pub fn run_auto_update_check(&self) {
        if !self.config.load().disable_auto_update_check {
            // Check for hachimi updates first, then translations
            // Don't auto check for tl updates if it's not up to date
            self.updater.clone().check_for_updates(|new_update| {
                let hachimi = Hachimi::instance();
                if !new_update && !hachimi.config.load().translator_mode {
                    hachimi.tl_updater.clone().check_for_updates(false);
                }
            });
        }
    }
}

fn default_serde_instance<'a, T: Deserialize<'a>>() -> Option<T> {
    let empty_data = std::iter::empty::<((), ())>();
    let empty_deserializer = serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(empty_data);
    T::deserialize(empty_deserializer).ok()
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default)]
    pub enable_file_logging: bool,
    #[serde(default)]
    pub translator_mode: bool,
    #[serde(default)]
    pub disable_gui: bool,
    #[serde(default)]
    pub disable_gui_once: bool,
    pub localized_data_dir: Option<String>,
    pub target_fps: Option<i32>,
    #[serde(default = "Config::default_open_browser_url")]
    pub open_browser_url: String,
    #[serde(default = "Config::default_virtual_res_mult")]
    pub virtual_res_mult: f32,
    pub translation_repo_index: Option<String>,
    #[serde(default)]
    pub skip_first_time_setup: bool,
    #[serde(default)]
    pub lazy_translation_updates: bool,
    #[serde(default)]
    pub disable_auto_update_check: bool,
    #[serde(default)]
    pub disable_translations: bool,
    #[serde(default = "Config::default_gui_scale")]
    pub gui_scale: f32,
    #[serde(default = "Config::default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default = "Config::default_render_scale")]
    pub render_scale: f32,
    #[serde(default)]
    pub msaa: crate::il2cpp::hook::umamusume::GraphicSettings::MsaaQuality,
    #[serde(default)]
    pub aniso_level: crate::il2cpp::hook::UnityEngine_CoreModule::Texture::AnisoLevel,
    #[serde(default)]
    pub shadow_resolution: crate::il2cpp::hook::umamusume::CameraData::ShadowResolution,
    #[serde(default)]
    pub graphics_quality: crate::il2cpp::hook::umamusume::GraphicSettings::GraphicsQuality,
    #[serde(default = "Config::default_story_choice_auto_select_delay")]
    pub story_choice_auto_select_delay: f32,
    #[serde(default = "Config::default_story_tcps_multiplier")]
    pub story_tcps_multiplier: f32,
    #[serde(default)]
    pub enable_ipc: bool,
    #[serde(default)]
    pub ipc_listen_all: bool,
    #[serde(default)]
    pub force_allow_dynamic_camera: bool,
    #[serde(default)]
    pub live_theater_allow_same_chara: bool,
    #[serde(default = "Config::default_live_vocals_swap")]
    pub live_vocals_swap: [i32; 6],
    #[serde(default)]
    pub skill_info_dialog: bool,
    #[serde(default)]
    pub homescreen_bgseason: crate::il2cpp::hook::umamusume::TimeUtil::BgSeason,
    pub sugoi_url: Option<String>,
    #[serde(default)]
    pub auto_translate_stories: bool,
    #[serde(default)]
    pub auto_translate_localize: bool,
    #[serde(default)]
    pub disable_skill_name_translation: bool,
    #[serde(default)]
    pub hide_ingame_ui_hotkey: bool,
    #[serde(default)]
    pub language: Language,
    #[serde(default = "Config::default_meta_index_url")]
    pub meta_index_url: String,
    pub physics_update_mode: Option<SpringUpdateMode>,
    #[serde(default = "Config::default_ui_animation_scale")]
    pub ui_animation_scale: f32,
    #[serde(default)]
    pub disabled_hooks: FnvHashSet<String>,

    // theme settings
    #[serde(default = "Config::default_ui_accent")]
    pub ui_accent_color: egui::Color32,
    #[serde(default = "Config::default_window_fill")]
    pub ui_window_fill: egui::Color32,
    #[serde(default = "Config::default_panel_fill")]
    pub ui_panel_fill: egui::Color32,
    #[serde(default = "Config::default_extreme_bg")]
    pub ui_extreme_bg_color: egui::Color32,
    #[serde(default = "Config::default_text_color")]
    pub ui_text_color: egui::Color32,
    #[serde(default = "Config::default_window_rounding")]
    pub ui_window_rounding: f32,

    #[cfg(target_os = "windows")]
    #[serde(flatten)]
    pub windows: hachimi_impl::Config,

    #[cfg(target_os = "android")]
    #[serde(flatten)]
    pub android: hachimi_impl::Config
}

impl Config {
    fn default_open_browser_url() -> String { "https://www.google.com/".to_owned() }
    fn default_virtual_res_mult() -> f32 { 1.0 }
    fn default_ui_scale() -> f32 { 1.0 }
    fn default_render_scale() -> f32 { 1.0 }
    fn default_gui_scale() -> f32 { 1.0 }
    fn default_story_choice_auto_select_delay() -> f32 { 1.2 }
    fn default_story_tcps_multiplier() -> f32 { 3.0 }
    fn default_meta_index_url() -> String { "https://gitlab.com/umatl/hachimi-meta/-/raw/main/meta.json".to_owned() }
    fn default_ui_animation_scale() -> f32 { 1.0 }
    fn default_live_vocals_swap() -> [i32; 6] { [0; 6] }
    pub fn default_ui_accent() -> egui::Color32 { egui::Color32::from_rgb(100, 150, 240) }
    pub fn default_window_fill() -> egui::Color32 { egui::Color32::from_rgba_premultiplied(27, 27, 27, 220) }
    pub fn default_panel_fill() -> egui::Color32 { egui::Color32::from_rgba_premultiplied(27, 27, 27, 220) }
    pub fn default_extreme_bg() -> egui::Color32 { egui::Color32::from_rgb(15, 15, 15) }
    pub fn default_text_color() -> egui::Color32 { egui::Color32::from_gray(170) }
    pub fn default_window_rounding() -> f32 { 10.0 }
}

impl Default for Config {
    fn default() -> Self {
        default_serde_instance().expect("default instance")
    }
}

#[derive(Deserialize, Serialize, Default, Clone)]
pub struct OsOption<T> {
    #[cfg(target_os = "android")]
    android: Option<T>,

    #[cfg(target_os = "windows")]
    windows: Option<T>
}

impl<T> OsOption<T> {
    pub fn as_ref(&self) -> Option<&T> {
        #[cfg(target_os = "android")]
        return self.android.as_ref();

        #[cfg(target_os = "windows")]
        return self.windows.as_ref();
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Deserialize, Serialize)]
#[allow(non_camel_case_types)]
pub enum Language {
    #[serde(rename = "en")]
    English,

    #[serde(rename = "zh-tw")]
    TChinese,

    #[serde(rename = "zh-cn")]
    SChinese,

    #[serde(rename = "vi")]
    Vietnamese,

    #[serde(rename = "id")]
    Indonesian,

    #[serde(rename = "es")]
    Spanish
}

impl Default for Language {
    fn default() -> Self {
        let locale = sys_locale::get_locale().as_deref().unwrap_or("en").to_lowercase();
        if locale.contains("zh-hk") || locale.contains("zh-tw") || locale.contains("zh-hant") {
            Self::TChinese
        } else if locale.contains("zh") {
            Self::SChinese
        } else if locale.starts_with("vi") {
            Self::Vietnamese
        } else if locale.starts_with("id") {
            Self::Indonesian
        } else if locale.starts_with("es") {
            Self::Spanish
        } else {
            Self::English
        }
    }
}

impl Language {
    pub const CHOICES: &[(Self, &'static str)] = &[
        Self::English.choice(),
        Self::TChinese.choice(),
        Self::SChinese.choice(),
        Self::Vietnamese.choice(),
        Self::Indonesian.choice(),
        Self::Spanish.choice()
    ];

    pub fn set_locale(&self) {
        rust_i18n::set_locale(self.locale_str());
    }

    pub const fn locale_str(&self) -> &'static str {
        match self {
            Language::English => "en",
            Language::TChinese => "zh-tw",
            Language::SChinese => "zh-cn",
            Language::Vietnamese => "vi",
            Language::Indonesian => "id",
            Language::Spanish => "es"
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Language::English => "English",
            Language::TChinese => "繁體中文",
            Language::SChinese => "简体中文",
            Language::Vietnamese => "Tiếng Việt",
            Language::Indonesian => "Bahasa Indonesia",
            Language::Spanish => "Español (ES)"
        }
    }

    pub const fn choice(self) -> (Self, &'static str) {
        (self, self.name())
    }
}

#[derive(Default)]
pub struct LocalizedData {
    pub config: LocalizedDataConfig,
    path: Option<PathBuf>,

    pub localize_dict: FnvHashMap<String, String>,
    pub hashed_dict: FnvHashMap<u64, String>,
    pub text_data_dict: FnvHashMap<i32, FnvHashMap<i32, String>>, // {"category": {"index": "text"}}
    pub character_system_text_dict: FnvHashMap<i32, FnvHashMap<i32, String>>, // {"character_id": {"voice_id": "text"}}
    pub race_jikkyo_comment_dict: FnvHashMap<i32, String>, // {"id": "text"}
    pub race_jikkyo_message_dict: FnvHashMap<i32, String>, // {"id": "text"}
    assets_path: Option<PathBuf>,

    pub plural_form: plurals::Resolver,
    pub ordinal_form: plurals::Resolver,

    pub wrapper_penalties: Penalties
}

impl LocalizedData {
    fn new(config: &Config, data_dir: &Path) -> Result<LocalizedData, Error> {
        if config.disable_translations {
            return Ok(LocalizedData::default());
        }

        let path: Option<PathBuf>;
        let config: LocalizedDataConfig = if let Some(ld_dir) = &config.localized_data_dir {
            let ld_path = Path::new(data_dir).join(ld_dir);

            // Create .nomedia
            #[cfg(target_os = "android")]
            { _ = fs::OpenOptions::new().create_new(true).write(true).open(ld_path.join(".nomedia")); }

            let ld_config_path = ld_path.join("config.json");
            path = Some(ld_path);

            if fs::metadata(&ld_config_path).is_ok() {
                load_and_repair_json(&ld_config_path)?
            }
            else {
                warn!("Localized data config not found");
                LocalizedDataConfig::default()
            }
        }
        else {
            path = None;
            LocalizedDataConfig::default()
        };

        let plural_form = Self::parse_plural_form_or_default(&config.plural_form)?;
        let ordinal_form = Self::parse_plural_form_or_default(&config.ordinal_form)?;

        let wrapper_penalties = Self::parse_wrap_penalties_or_default(&config.wrapper_penalties);

        Ok(LocalizedData {
            localize_dict: Self::load_dict_static(&path, config.localize_dict.as_ref()).unwrap_or_default(),
            hashed_dict: Self::load_dict_static(&path, config.hashed_dict.as_ref()).unwrap_or_default(),
            text_data_dict: Self::load_dict_static(&path, config.text_data_dict.as_ref()).unwrap_or_default(),
            character_system_text_dict: Self::load_dict_static(&path, config.character_system_text_dict.as_ref()).unwrap_or_default(),
            race_jikkyo_comment_dict: Self::load_dict_static(&path, config.race_jikkyo_comment_dict.as_ref()).unwrap_or_default(),
            race_jikkyo_message_dict: Self::load_dict_static(&path, config.race_jikkyo_message_dict.as_ref()).unwrap_or_default(),
            assets_path: path.as_ref()
                .map(|p| config.assets_dir.as_ref()
                    .map(|dir| p.join(dir))
                )
                .unwrap_or_default(),

            plural_form,
            ordinal_form,

            wrapper_penalties,

            config,
            path
        })
    }

    fn load_dict_static_ex<T: DeserializeOwned, P: AsRef<Path>>(ld_path_opt: &Option<PathBuf>, rel_path_opt: Option<P>, silent_fs_error: bool) -> Option<T> {
        let Some(ld_path) = ld_path_opt else {
            return None;
        };
        let Some(rel_path) = rel_path_opt else {
            return None;
        };

        let path = ld_path.join(rel_path);
        let json = match fs::read_to_string(&path) {
            Ok(v) => v,
            Err(e) => {
                if !silent_fs_error {
                    error!("Failed to read '{}': {}", path.display(), e);
                }
                return None;
            }
        };

        let dict = match serde_json::from_str::<T>(&json) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to parse '{}': {}", path.display(), e);
                return None;
            }
        };

        Some(dict)
    }

    fn load_dict_static<T: DeserializeOwned, P: AsRef<Path>>(ld_path_opt: &Option<PathBuf>, rel_path_opt: Option<P>) -> Option<T> {
        Self::load_dict_static_ex(ld_path_opt, rel_path_opt, false)
    }

    pub fn load_dict<T: DeserializeOwned, P: AsRef<Path>>(&self, rel_path_opt: Option<P>) -> Option<T> {
        Self::load_dict_static(&self.path, rel_path_opt)
    }

    pub fn load_assets_dict<T: DeserializeOwned, P: AsRef<Path>>(&self, rel_path_opt: Option<P>) -> Option<T> {
        Self::load_dict_static_ex(&self.assets_path, rel_path_opt, true)
    }

    fn parse_plural_form_or_default(opt: &Option<String>) -> Result<plurals::Resolver, Error> {
        if let Some(plural_form) = opt {
            Ok(plurals::Resolver::Expr(plurals::Ast::parse(plural_form)?))
        }
        else {
            Ok(plurals::Resolver::Function(|_| 0))
        }
    }

    fn parse_wrap_penalties_or_default(opt: &Option<PenaltiesConfig>) -> Penalties {
        let Some(cfg) = opt else {
            return Penalties::new()
        };
        Penalties {
            nline_penalty: cfg.nline_penalty,
            overflow_penalty: cfg.overflow_penalty,
            short_last_line_fraction: cfg.short_last_line_fraction,
            short_last_line_penalty: cfg.short_last_line_penalty,
            hyphen_penalty: cfg.hyphen_penalty
        }
    }

    pub fn get_assets_path<P: AsRef<Path>>(&self, rel_path: P) -> Option<PathBuf> {
        self.assets_path.as_ref().map(|p| p.join(rel_path))
    }

    pub fn get_data_path<P: AsRef<Path>>(&self, rel_path: P) -> Option<PathBuf> {
        self.path.as_ref().map(|p| p.join(rel_path))
    }

    pub fn load_asset_metadata<P: AsRef<Path>>(&self, rel_path: P) -> AssetMetadata {
        let mut path = rel_path.as_ref().to_owned();
        path.set_extension("json");
        self.load_assets_dict(Some(path)).unwrap_or_else(|| AssetInfo::<()>::default()).metadata()
    }

    pub fn load_asset_info<P: AsRef<Path>, T: DeserializeOwned>(&self, rel_path: P) -> AssetInfo<T> {
        let mut path = rel_path.as_ref().to_owned();
        path.set_extension("json");
        self.load_assets_dict(Some(path)).unwrap_or_else(|| AssetInfo::default())
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct LocalizedDataConfig {
    pub localize_dict: Option<String>,
    pub hashed_dict: Option<String>,
    pub text_data_dict: Option<String>,
    pub character_system_text_dict: Option<String>,
    pub race_jikkyo_comment_dict: Option<String>,
    pub race_jikkyo_message_dict: Option<String>,
    pub assets_dir: Option<String>,
    #[serde(default)]
    pub extra_asset_bundle: OsOption<String>,
    pub replacement_font_name: Option<String>,

    pub plural_form: Option<String>,
    pub ordinal_form: Option<String>,
    #[serde(default)]
    pub ordinal_types: Vec<String>,
    #[serde(default)]
    pub months: Vec<String>,
    pub month_text_format: Option<String>,

    #[serde(default)]
    pub use_text_wrapper: bool,
    // Predefined line widths are counts of cjk characters.
    // 1 cjk char = 2 columns, so setting this value to 2 replicates the default behaviour.
    pub line_width_multiplier: Option<f32>,
    #[serde(default)]
    pub systext_cue_lines: FnvHashMap<String, i32>,
    pub wrapper_penalties: Option<PenaltiesConfig>,

    #[serde(default)]
    pub auto_adjust_story_clip_length: bool,
    pub story_line_count_offset: Option<i32>,
    pub text_frame_line_spacing_multiplier: Option<f32>,
    pub text_frame_font_size_multiplier: Option<f32>,
    #[serde(default)]
    pub skill_formatting: SkillFormatting,
    #[serde(default)]
    pub text_common_allow_overflow: bool,
    #[serde(default)]
    pub now_loading_comic_title_ellipsis: bool,

    #[serde(default)]
    pub remove_ruby: bool,
    pub character_note_top_gallery_button: Option<UITextConfig>,
    pub character_note_top_talk_gallery_button: Option<UITextConfig>,

    pub news_url: Option<String>,

    // RESERVED
    #[serde(default)]
    pub _debug: i32
}

#[derive(Deserialize, Serialize, Clone)]
pub struct UITextConfig {
    pub text: Option<String>,
    pub font_size: Option<i32>,
    pub line_spacing: Option<f32>
}

impl Default for LocalizedDataConfig {
    fn default() -> Self {
        default_serde_instance().expect("default instance")
    }
}

#[derive(Deserialize)]
pub struct AssetInfo<T> {
    #[cfg(target_os = "android")]
    #[serde(default)]
    android: AssetMetadata,

    #[cfg(target_os = "windows")]
    #[serde(default)]
    windows: AssetMetadata,

    pub data: Option<T>
}

// Can't derive(Default), see rust-lang/rust#26925
impl<T> Default for AssetInfo<T> {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "android")]
            android: Default::default(),

            #[cfg(target_os = "windows")]
            windows: Default::default(),

            data: None
        }
    }
}

impl<T> AssetInfo<T> {
    pub fn metadata(self) -> AssetMetadata {
        #[cfg(target_os = "android")]
        return self.android;

        #[cfg(target_os = "windows")]
        return self.windows;
    }

    pub fn metadata_ref(&self) -> &AssetMetadata {
        #[cfg(target_os = "android")]
        return &self.android;

        #[cfg(target_os = "windows")]
        return &self.windows;
    }
}

#[derive(Deserialize, Clone, Default)]
pub struct AssetMetadata {
    pub bundle_name: Option<String>
}

#[derive(Deserialize, Serialize, Clone)]
pub struct PenaltiesConfig {
    nline_penalty: usize,
    overflow_penalty: usize,
    short_last_line_fraction: usize,
    short_last_line_penalty: usize,
    hyphen_penalty: usize
}

#[derive(Deserialize, Serialize, Clone)]
pub struct SkillFormatting {
    #[serde(default = "SkillFormatting::default_length")]
    pub name_length: i32,
    #[serde(default = "SkillFormatting::default_length")]
    pub desc_length: i32,
    #[serde(default = "SkillFormatting::default_lines")]
    pub name_short_lines: i32,

    #[serde(default = "SkillFormatting::default_mult")]
    pub name_short_mult: f32,
    #[serde(default = "SkillFormatting::default_mult")]
    pub name_sp_mult: f32,
}
impl SkillFormatting {
    fn default_length() -> i32 { 18 }
    fn default_lines() -> i32 { 1 }
    fn default_mult() -> f32 { 1.0 }
}

impl Default for SkillFormatting {
    fn default() -> Self {
        SkillFormatting {
            name_length: 13,
            desc_length: 18,
            name_short_lines: 1,
            name_short_mult: 1.0,
            name_sp_mult: 1.0 }
    }
}