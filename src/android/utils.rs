use jni::objects::JObject;
use jni::JNIEnv;

pub fn get_context(env: &JNIEnv) -> JObject {
    let activity_thread_class = env
        .find_class("android/app/ActivityThread")
        .expect("Failed to find ActivityThread class");
    let activity_thread_obj = env
        .call_static_method(
            activity_thread_class,
            "currentActivityThread",
            "()Landroid/app/ActivityThread;",
            &[],
        )
        .expect("Failed to get current ActivityThread")
        .l()
        .unwrap();

    let context_obj = env
        .call_method(
            activity_thread_obj,
            "getApplication",
            "()Landroid/app/Application;",
            &[],
        )
        .expect("Failed to get Application context")
        .l()
        .unwrap();

    context_obj
}

pub fn get_device_api_level(env: *mut jni::sys::JNIEnv) -> i32 {
    let mut env = unsafe { JNIEnv::from_raw(env).unwrap() };
    env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")
        .unwrap()
        .i()
        .unwrap()
}