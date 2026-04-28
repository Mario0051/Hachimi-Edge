use std::{fs, io::Write, time::{SystemTime, UNIX_EPOCH}, sync::atomic::{AtomicI32, Ordering}};
use rmpv::Value;
use ureq::Agent;
use crate::core::Hachimi;

#[cfg(target_os = "windows")]
use windows::{
    core::HSTRING,
    Foundation::DateTime,
    UI::Notifications::{ToastNotificationManager, ScheduledToastNotification, ToastTemplateType}
};

static LEADER_CHARA_ID: AtomicI32 = AtomicI32::new(1001);

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