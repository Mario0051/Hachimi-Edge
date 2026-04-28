use std::{fs, io::Write, time::{SystemTime, UNIX_EPOCH}, sync::{atomic::{AtomicI32, Ordering}, Mutex}};
use fnv::FnvHashSet;
use once_cell::sync::Lazy;
use rmpv::Value;
use rand::seq::IndexedRandom;
use ureq::Agent;
use crate::core::Hachimi;

#[cfg(target_os = "windows")]
use windows::{
    core::HSTRING,
    Foundation::DateTime,
    UI::Notifications::{ToastNotificationManager, ScheduledToastNotification, ToastTemplateType}
};

static REAL_OWNED_CHARAS: Lazy<Mutex<FnvHashSet<i32>>> = Lazy::new(|| Mutex::default());
static REAL_OWNED_DRESSES: Lazy<Mutex<FnvHashSet<i32>>> = Lazy::new(|| Mutex::default());
static REAL_OWNED_SONGS: Lazy<Mutex<FnvHashSet<i32>>> = Lazy::new(|| Mutex::default());
static LIVE_SAVE_INFO_MAP: Lazy<Mutex<std::collections::HashMap<i32, Value>>> = Lazy::new(|| Mutex::default());
static LEADER_CHARA_ID: AtomicI32 = AtomicI32::new(1001);

fn v_int(i: i32) -> Value { Value::Integer(i.into()) }
fn v_str(s: &str) -> Value { Value::String(s.into()) }

pub fn dump_msgpack(data: &[u8], suffix: &str) {
    let hachimi = Hachimi::instance();
    let dump_dir = hachimi.get_data_path("msgpack_dump");
    let _ = fs::create_dir_all(&dump_dir);
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    let file_path = dump_dir.join(format!("{}{}.msgpack", timestamp, suffix));
    if let Ok(mut file) = fs::File::create(file_path) {
        let _ = file.write_all(data);
    }
}

pub fn modify_request(data: &[u8]) -> Option<Vec<u8>> {
    let config = Hachimi::instance().config.load();
    if !config.unlock_live_chara { return None; }

    let mut cursor = std::io::Cursor::new(data);
    let mut val = match rmpv::decode::read_value(&mut cursor) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let mut modified = false;

    if let Value::Map(ref mut map) = val {
        for (k, v) in map.iter_mut() {
            if k.as_str() == Some("live_theater_save_info") {
                if let Value::Map(ref mut info_map) = v {
                    if process_theater_save(info_map) {
                        modified = true;
                    }
                }
            }
        }
    }

    if modified {
        let mut out = Vec::new();
        if rmpv::encode::write_value(&mut out, &val).is_ok() {
            return Some(out);
        }
    }
    None
}

pub fn modify_response(data: &[u8]) -> Option<Vec<u8>> {
    let config = Hachimi::instance().config.load();
    if !config.unlock_live_chara { return None; }

    let mut cursor = std::io::Cursor::new(data);
    let mut val = match rmpv::decode::read_value(&mut cursor) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let mut modified = false;

    if let Value::Map(ref mut map) = val {
        for (k, v) in map.iter_mut() {
            if k.as_str() == Some("data") {
                if let Value::Map(ref mut data_map) = v {
                    if process_chara_list(data_map) { modified = true; }
                    if process_chara_profile(data_map) { modified = true; }
                    if process_card_list(data_map) { modified = true; }
                    if process_cloth_list(data_map) { modified = true; }
                    if process_music_list(data_map) { modified = true; }
                    if process_release_card(data_map) { modified = true; }
                    if process_save_info(data_map) { modified = true; }
                }
            }
        }
    }

    if modified {
        let mut out = Vec::new();
        if rmpv::encode::write_value(&mut out, &val).is_ok() {
            return Some(out);
        }
    }
    None
}

fn process_theater_save(info_map: &mut Vec<(Value, Value)>) -> bool {
    let mut music_id = 0;
    for (k, v) in info_map.iter() {
        if k.as_str() == Some("music_id") {
            music_id = v.as_i64().unwrap_or(0) as i32;
        }
    }

    let mut valid = true;
    let mut member_info_index = None;
    for (i, (k, v)) in info_map.iter().enumerate() {
        if k.as_str() == Some("member_info_array") {
            member_info_index = Some(i);
            if let Value::Array(members) = v {
                let charas = REAL_OWNED_CHARAS.lock().unwrap();
                let dresses = REAL_OWNED_DRESSES.lock().unwrap();
                let default_dresses = crate::il2cpp::sql::get_default_dress_ids();

                for member in members {
                    if let Value::Map(mmap) = member {
                        for (mk, mv) in mmap {
                            if mk.as_str() == Some("chara_id") {
                                let c_id = mv.as_i64().unwrap_or(0) as i32;
                                if c_id > 0 && !charas.contains(&c_id) { valid = false; }
                            }
                            if mk.as_str() == Some("dress_id") || mk.as_str() == Some("dress_id2") {
                                let d_id = mv.as_i64().unwrap_or(0) as i32;
                                if d_id > 0 && !dresses.contains(&d_id) && !default_dresses.contains(&d_id) {
                                    valid = false;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if valid && REAL_OWNED_SONGS.lock().unwrap().contains(&music_id) {
        return false;
    }

    if let Some(idx) = member_info_index {
        if let Value::Array(members) = &mut info_map[idx].1 {
            let charas = REAL_OWNED_CHARAS.lock().unwrap();
            let dresses = REAL_OWNED_DRESSES.lock().unwrap();
            let default_dresses = crate::il2cpp::sql::get_default_dress_ids();
            let mobs = crate::il2cpp::sql::get_all_mob_ids();
            let mut rng = rand::rng();

            for member in members.iter_mut() {
                if let Value::Map(mmap) = member {
                    let mut c_id = 0;
                    for (mk, mv) in mmap.iter() {
                        if mk.as_str() == Some("chara_id") { c_id = mv.as_i64().unwrap_or(0) as i32; }
                    }

                    for (mk, mv) in mmap.iter_mut() {
                        match mk.as_str() {
                            Some("chara_id") => {
                                if c_id > 0 && !charas.contains(&c_id) { *mv = v_int(0); }
                            }
                            Some("mob_id") => {
                                if c_id > 0 && !charas.contains(&c_id) {
                                    if let Some(&m_id) = mobs.choose(&mut rng) { *mv = v_int(m_id); }
                                }
                            }
                            Some("dress_id") | Some("dress_id2") => {
                                let d_id = mv.as_i64().unwrap_or(0) as i32;
                                if !dresses.contains(&d_id) && !default_dresses.contains(&d_id) {
                                    *mv = v_int(7);
                                } else if c_id > 0 && !charas.contains(&c_id) {
                                    *mv = v_int(7);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    true
}

fn process_chara_list(data_map: &mut Vec<(Value, Value)>) -> bool {
    let mut modified = false;
    let mut existing_map = std::collections::HashMap::new();
    let mut target_idx = None;

    for (i, (k, v)) in data_map.iter().enumerate() {
        if k.as_str() == Some("chara_list") {
            target_idx = Some(i);
            if let Value::Array(arr) = v {
                let mut owned = REAL_OWNED_CHARAS.lock().unwrap();
                owned.clear();
                for item in arr {
                    if let Value::Map(cmap) = item {
                        for (ck, cv) in cmap {
                            if ck.as_str() == Some("chara_id") {
                                let c_id = cv.as_i64().unwrap_or(0) as i32;
                                owned.insert(c_id);
                                existing_map.insert(c_id, item.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(idx) = target_idx {
        let mut new_arr = Vec::new();
        for id in crate::il2cpp::sql::get_all_chara_ids() {
            if let Some(item) = existing_map.get(&id) {
                new_arr.push(item.clone());
            } else {
                new_arr.push(Value::Map(vec![
                    (v_str("chara_id"), v_int(id)),
                    (v_str("training_num"), v_int(0)),
                    (v_str("love_point"), v_int(0)),
                    (v_str("fan"), v_int(1)),
                    (v_str("max_grade"), v_int(0)),
                    (v_str("dress_id"), v_int(2)),
                    (v_str("mini_dress_id"), v_int(2)),
                    (v_str("love_point_pool"), v_int(0)),
                ]));
            }
        }
        data_map[idx].1 = Value::Array(new_arr);
        modified = true;
    }
    modified
}

fn process_chara_profile(data_map: &mut Vec<(Value, Value)>) -> bool {
    let mut modified = false;
    let mut existing_map = std::collections::HashMap::new();
    let mut target_idx = None;

    for (i, (k, v)) in data_map.iter().enumerate() {
        if k.as_str() == Some("chara_profile_array") {
            target_idx = Some(i);
            if let Value::Array(arr) = v {
                for item in arr {
                    if let Value::Map(cmap) = item {
                        for (ck, cv) in cmap {
                            if ck.as_str() == Some("chara_id") {
                                existing_map.insert(cv.as_i64().unwrap_or(0) as i32, item.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(idx) = target_idx {
        let mut new_arr = Vec::new();
        for id in crate::il2cpp::sql::get_all_chara_ids() {
            if let Some(item) = existing_map.get(&id) {
                new_arr.push(item.clone());
            } else {
                new_arr.push(Value::Map(vec![
                    (v_str("chara_id"), v_int(id)),
                    (v_str("data_id"), v_int(1)),
                    (v_str("new_flag"), v_int(0)),
                ]));
            }
        }
        data_map[idx].1 = Value::Array(new_arr);
        modified = true;
    }
    modified
}

fn process_card_list(data_map: &mut Vec<(Value, Value)>) -> bool {
    let mut modified = false;
    let mut existing_map = std::collections::HashMap::new();
    let mut target_idx = None;

    for (i, (k, v)) in data_map.iter().enumerate() {
        if k.as_str() == Some("card_list") {
            target_idx = Some(i);
            if let Value::Array(arr) = v {
                for item in arr {
                    if let Value::Map(cmap) = item {
                        for (ck, cv) in cmap {
                            if ck.as_str() == Some("card_id") {
                                existing_map.insert(cv.as_i64().unwrap_or(0) as i32, item.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(idx) = target_idx {
        let mut new_arr = Vec::new();
        for (id, rarity) in crate::il2cpp::sql::get_all_cards() {
            if let Some(mut item) = existing_map.get(&id).cloned() {
                if let Value::Map(ref mut cmap) = item {
                    for (ck, cv) in cmap.iter_mut() {
                        if ck.as_str() == Some("rarity") && cv.as_i64().unwrap_or(0) < 3 {
                            *cv = v_int(3);
                        }
                    }
                }
                new_arr.push(item);
            } else {
                new_arr.push(Value::Map(vec![
                    (v_str("null"), v_int(1)),
                    (v_str("card_id"), v_int(id)),
                    (v_str("rarity"), v_int(rarity.max(3))),
                    (v_str("talent_level"), v_int(1)),
                    (v_str("create_time"), v_str("2022-07-01 12:00:00")),
                    (v_str("skill_data_array"), Value::Array(vec![])),
                ]));
            }
        }
        data_map[idx].1 = Value::Array(new_arr);
        modified = true;
    }
    modified
}

fn process_release_card(data_map: &mut Vec<(Value, Value)>) -> bool {
    let mut modified = false;
    for (k, v) in data_map.iter_mut() {
        if k.as_str() == Some("release_card_array") {
            let mut new_arr = Vec::new();
            for (id, _) in crate::il2cpp::sql::get_all_cards() {
                new_arr.push(v_int(id));
            }
            *v = Value::Array(new_arr);
            modified = true;
        }
    }
    modified
}

fn process_cloth_list(data_map: &mut Vec<(Value, Value)>) -> bool {
    let mut modified = false;
    for (k, v) in data_map.iter_mut() {
        if k.as_str() == Some("cloth_list") {
            if let Value::Array(arr) = v {
                let mut owned = REAL_OWNED_DRESSES.lock().unwrap();
                owned.clear();
                for item in arr {
                    if let Value::Map(cmap) = item {
                        for (ck, cv) in cmap {
                            if ck.as_str() == Some("cloth_id") {
                                owned.insert(cv.as_i64().unwrap_or(0) as i32);
                            }
                        }
                    }
                }
            }

            let mut new_arr = Vec::new();
            for id in crate::il2cpp::sql::get_all_dress_ids() {
                new_arr.push(Value::Map(vec![(v_str("cloth_id"), v_int(id))]));
            }
            *v = Value::Array(new_arr);
            modified = true;
        }
    }
    modified
}

fn process_music_list(data_map: &mut Vec<(Value, Value)>) -> bool {
    let mut modified = false;
    for (k, v) in data_map.iter_mut() {
        if k.as_str() == Some("music_list") {
            if let Value::Array(arr) = v {
                let mut owned = REAL_OWNED_SONGS.lock().unwrap();
                owned.clear();
                for item in arr {
                    if let Value::Map(cmap) = item {
                        for (ck, cv) in cmap {
                            if ck.as_str() == Some("music_id") {
                                owned.insert(cv.as_i64().unwrap_or(0) as i32);
                            }
                        }
                    }
                }
            }

            let mut new_arr = Vec::new();
            for id in crate::il2cpp::sql::get_all_music_ids() {
                new_arr.push(Value::Map(vec![
                    (v_str("music_id"), v_int(id)),
                    (v_str("acquisition_time"), v_str("2022-07-01 12:00:00"))
                ]));
            }
            *v = Value::Array(new_arr);
            modified = true;
        }
    }
    modified
}

fn process_save_info(data_map: &mut Vec<(Value, Value)>) -> bool {
    let mut modified = false;
    for (k, v) in data_map.iter_mut() {
        if k.as_str() == Some("live_theater_save_info_array") {
            if let Value::Array(arr) = v {
                let mut save_map = LIVE_SAVE_INFO_MAP.lock().unwrap();
                save_map.clear();
                for item in arr.iter() {
                    if let Value::Map(cmap) = item {
                        for (ck, cv) in cmap {
                            if ck.as_str() == Some("music_id") {
                                save_map.insert(cv.as_i64().unwrap_or(0) as i32, item.clone());
                            }
                        }
                    }
                }
            }
            modified = true;
        }
    }
    modified
}

pub fn read_response(data: &[u8]) {
    let config = Hachimi::instance().config.load();
    if !config.notification_tp && !config.notification_rp && !config.notification_jobs {
        return;
    }

    let mut cursor = std::io::Cursor::new(data);
    if let Ok(val) = rmpv::decode::read_value(&mut cursor) {
        if let Value::Map(map) = val {
            for (k, v) in map {
                if k.as_str() == Some("data") {
                    if let Value::Map(data_map) = v {
                        parse_data_map(&data_map, &config);
                    }
                }
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn parse_data_map(_data_map: &[(Value, Value)], _config: &crate::core::hachimi::Config) {
}

#[cfg(target_os = "windows")]
fn parse_data_map(data_map: &[(Value, Value)], config: &crate::core::hachimi::Config) {
    let mut tp_recovery_time = None;
    let mut rp_recovery_time = None;
    let mut jobs_going_info_array: Option<&Vec<Value>> = None;

    for (k, v) in data_map {
        match k.as_str() {
            Some("user_info") => {
                if let Value::Map(user_map) = v {
                    for (uk, uv) in user_map {
                        if uk.as_str() == Some("leader_chara_id") {
                            if let Some(id) = uv.as_i64() {
                                LEADER_CHARA_ID.store(id as i32, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            Some("tp_info") => {
                if let Value::Map(tp_map) = v {
                    for (tk, tv) in tp_map {
                        if tk.as_str() == Some("max_recovery_time") {
                            tp_recovery_time = tv.as_i64();
                        }
                    }
                }
            }
            Some("rp_info") => {
                if let Value::Map(rp_map) = v {
                    for (rk, rv) in rp_map {
                        if rk.as_str() == Some("max_recovery_time") {
                            rp_recovery_time = rv.as_i64();
                        }
                    }
                }
            }
            Some("jobs_going_info_array") => {
                if let Value::Array(arr) = v {
                    jobs_going_info_array = Some(arr);
                }
            }
            Some("jobs_load_info") => {
                if let Value::Map(load_info_map) = v {
                    for (lk, lv) in load_info_map {
                        if lk.as_str() == Some("jobs_going_info_array") {
                            if let Value::Array(arr) = lv {
                                jobs_going_info_array = Some(arr);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let leader_id = LEADER_CHARA_ID.load(Ordering::Relaxed);
    let chara_name = crate::il2cpp::sql::get_master_text(6, leader_id).unwrap_or_default();
    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;

    let schedule_windows_toast_custom = |time: i64, tag: &str, title: &str, content: &str| {
        if time > now_secs {
            let epoch_diff = 11644473600_i64;
            let file_time = (time + epoch_diff) * 10_000_000;
            let toast_xml = ToastNotificationManager::GetTemplateContent(ToastTemplateType::ToastText02).unwrap();
            let text_nodes = toast_xml.GetElementsByTagName(&HSTRING::from("text")).unwrap();
            text_nodes.Item(0).unwrap().AppendChild(&toast_xml.CreateTextNode(&HSTRING::from(title)).unwrap()).unwrap();
            text_nodes.Item(1).unwrap().AppendChild(&toast_xml.CreateTextNode(&HSTRING::from(content)).unwrap()).unwrap();

            let delivery_time = DateTime { UniversalTime: file_time };
            let scheduled_toast = ScheduledToastNotification::CreateScheduledToastNotification(&toast_xml, delivery_time).unwrap();
            let _ = scheduled_toast.SetTag(&HSTRING::from(tag));
            let _ = scheduled_toast.SetGroup(&HSTRING::from("Generic"));

            let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from("Cygames.Gallop")).unwrap();
            let _ = notifier.AddToSchedule(&scheduled_toast);
        }
    };

    let schedule_windows_toast = |time: i64, tag: &str, msg_category: i32| {
        let content = crate::il2cpp::sql::get_master_text(msg_category, leader_id)
            .unwrap_or_default()
            .replace("\\n", "\n");
        schedule_windows_toast_custom(time, tag, &chara_name, &content);
    };

    if config.notification_tp {
        if let Some(time) = tp_recovery_time {
            schedule_windows_toast(time, "TP", 184);
        }
    }

    if config.notification_rp {
        if let Some(time) = rp_recovery_time {
            schedule_windows_toast(time, "RP", 185);
        }
    }

    if config.notification_jobs {
        if let Some(jobs_array) = jobs_going_info_array {
            if let Ok(notifier) = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from("Cygames.Gallop")) {
                if let Ok(scheduled) = notifier.GetScheduledToastNotifications() {
                    let size = scheduled.Size().unwrap_or(0);
                    for i in 0..size {
                        if let Ok(toast) = scheduled.GetAt(i) {
                            if let Ok(tag) = toast.Tag() {
                                if tag.to_string_lossy().starts_with("Jobs_") {
                                    let _ = notifier.RemoveFromSchedule(&toast);
                                }
                            }
                        }
                    }
                }
            }

            for (index, job_val) in jobs_array.iter().enumerate() {
                if let Value::Map(job_map) = job_val {
                    let mut reward_id = 0;
                    let mut end_time_str = "";
                    let mut leader_card_id = 0;

                    for (jk, jv) in job_map {
                        match jk.as_str() {
                            Some("jobs_reward_id") => reward_id = jv.as_i64().unwrap_or(0) as i32,
                            Some("end_time") => end_time_str = jv.as_str().unwrap_or(""),
                            Some("attend_card_info_array") => {
                                if let Value::Array(card_arr) = jv {
                                    if let Some(Value::Map(card_map)) = card_arr.get(0) {
                                        for (ck, cv) in card_map {
                                            if ck.as_str() == Some("card_id") {
                                                leader_card_id = cv.as_i64().unwrap_or(0) as i32;
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    if leader_card_id > 0 && !end_time_str.is_empty() {
                        let chara_id = (leader_card_id as f32 * 0.01).floor() as i32;
                        use chrono::TimeZone;

                        if let Ok(naive_dt) = chrono::NaiveDateTime::parse_from_str(end_time_str, "%Y-%m-%d %H:%M:%S") {
                            if let Some(local_dt) = chrono::Local.from_local_datetime(&naive_dt).single() {
                                let end_time_secs = local_dt.timestamp();

                                if let Some((place_id, genre_id)) = crate::il2cpp::sql::get_jobs_info(reward_id) {
                                    if let Some(track_id) = crate::il2cpp::sql::get_jobs_place_race_track_id(place_id) {
                                        let track_name = crate::il2cpp::sql::get_master_text(34, track_id).unwrap_or_default();
                                        let genre_name = crate::il2cpp::sql::get_master_text(357, genre_id).unwrap_or_default();

                                        let content_template = crate::il2cpp::sql::get_master_text(360, chara_id).unwrap_or_default().replace("\\n", "\n");
                                        let placename = format!("【{}】{}", track_name, genre_name);
                                        let content = content_template.replace("<jobs_placename>", &placename);

                                        let title = crate::il2cpp::sql::get_master_text(6, chara_id).unwrap_or_default();
                                        let tag = format!("Jobs_{}", index);

                                        schedule_windows_toast_custom(end_time_secs, &tag, &title, &content);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn broadcast_msgpack(data: &[u8], is_request: bool) {
    let config = crate::core::Hachimi::instance().config.load();
    if !config.msgpack_notifier { return; }
    if is_request && !config.msgpack_notifier_request { return; }

    let host = config.msgpack_notifier_host.clone();
    let timeout = config.msgpack_notifier_connection_timeout_ms as u64;
    let endpoint = if is_request { "/notify/request" } else { "/notify/response" };
    let url = format!("{}{}", host, endpoint);
    let payload = data.to_vec();

    std::thread::spawn(move || {
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_millis(timeout)))
            .build()
            .into();

        let res = agent.post(&url)
            .header("Content-Type", "application/x-msgpack")
            .send(payload);

        if let Err(e) = res {
            let config = crate::core::Hachimi::instance().config.load();
            if config.msgpack_notifier_print_error {
                log::warn!("MsgPack Notifier HTTP Error: {}", e);
            }
        }
    });
}