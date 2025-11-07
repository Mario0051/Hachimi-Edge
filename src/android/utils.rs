use jni::objects::{GlobalRef, JObject};
use jni::JNIEnv;

pub fn get_context(env: &mut JNIEnv) -> GlobalRef {
    let unity_player_class = env
        .find_class("com/unity3d/player/UnityPlayer")
        .expect("Failed to find com.unity3d.player.UnityPlayer class");

    let activity_obj = env
        .get_static_field(
            unity_player_class,
            "currentActivity",
            "Landroid/app/Activity;",
        )
        .expect("Failed to get static field 'currentActivity'")
        .l()
        .unwrap();

    env.new_global_ref(activity_obj)
        .expect("Failed to create GlobalRef for context")
}

pub fn get_device_api_level(env: *mut jni::sys::JNIEnv) -> i32 {
    let mut env = unsafe { JNIEnv::from_raw(env).unwrap() };
    env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")
        .unwrap()
        .i()
        .unwrap()
}