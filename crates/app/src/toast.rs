//! 安卓原生「提示」（Toast）。Toast 只能在 Java 主线程的 Looper 上弹，
//! 所以把 AndroidApp 存下来，需要时投递过去。

use std::sync::OnceLock;

use jni::objects::{JObject, JValue};
use jni::{jni_sig, jni_str};
use winit::platform::android::activity::AndroidApp;

static APP: OnceLock<AndroidApp> = OnceLock::new();

/// 在 `android_main` 里绑定一次；重复调用取第一次的。
pub fn bind(app: &AndroidApp) {
    let _ = APP.set(app.clone());
}

pub fn show(message: String) {
    let Some(app) = APP.get() else {
        log::error!("AndroidApp 未绑定，提示发不出去: {message}");
        return;
    };
    let worker = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        if let Err(e) = toast(&worker, &message) {
            log::error!("提示弹出失败: {e}");
        }
    }));
}

fn toast(
    app: &AndroidApp,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) };
    vm.attach_current_thread(|env| {
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) };
        let text: JObject = env.new_string(message)?.into();
        let made = env.call_static_method(
            jni_str!("android/widget/Toast"),
            jni_str!("makeText"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/CharSequence;I)Landroid/widget/Toast;"),
            &[
                JValue::Object(&activity),
                JValue::Object(&text),
                JValue::Int(0), // Toast.LENGTH_SHORT
            ],
        )?;
        let toast = made.l()?;
        env.call_method(&toast, jni_str!("show"), jni_sig!("()V"), &[])?;
        Ok(())
    })
}
