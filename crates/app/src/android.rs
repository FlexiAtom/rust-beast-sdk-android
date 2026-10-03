//! 安卓平台侧：原生「提示」（Toast）+ 把 GameActivity 的输入法缓冲搬进 egui。

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::OnceLock;

use eframe::egui;
use jni::objects::{JObject, JValue};
use jni::{jni_sig, jni_str};
use winit::platform::android::activity::AndroidApp;

static APP: OnceLock<AndroidApp> = OnceLock::new();

/// 状态栏高度（物理像素）。GameActivity 的 SurfaceView 铺满整屏，
/// egui 的内容从 y=0 画起会被状态栏压住，所以首行要空出这段。
/// 0 = 还没查到或查不到，先按 0 画，下一帧补正。
static TOP_INSET_PX: AtomicI32 = AtomicI32::new(0);

/// 在 `android_main` 里绑定一次；重复调用取第一次的。
pub fn bind(app: &AndroidApp) {
    let _ = APP.set(app.clone());
    let worker = app.clone();
    app.run_on_java_main_thread(Box::new(move || match status_bar_px(&worker) {
        Ok(px) => TOP_INSET_PX.store(px, Ordering::Relaxed),
        Err(e) => log::warn!("状态栏高度查不到，顶部留白按 0 处理: {e}"),
    }));
}

pub fn top_inset_px() -> i32 {
    TOP_INSET_PX.load(Ordering::Relaxed)
}

/// 平台自己的 `android:dimen/status_bar_height`，不走资源合并，apk 里没有这个资源也能读到。
fn status_bar_px(app: &AndroidApp) -> Result<i32, Box<dyn std::error::Error>> {
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) };
    // 闭包的错误类型要写死：`From<jni::Error>` 有多个候选实现，外面又是 map_err 推不出来
    vm.attach_current_thread(|env| -> jni::errors::Result<i32> {
        let activity =
            unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) };
        let res = env
            .call_method(
                &activity,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?;
        let name: JObject = env.new_string("status_bar_height")?.into();
        let kind: JObject = env.new_string("dimen")?.into();
        let pkg: JObject = env.new_string("android")?.into();
        let id = env
            .call_method(
                &res,
                jni_str!("getIdentifier"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I"),
                &[
                    JValue::Object(&name),
                    JValue::Object(&kind),
                    JValue::Object(&pkg),
                ],
            )?
            .i()?;
        if id == 0 {
            return Ok(0);
        }
        let px = env
            .call_method(
                &res,
                jni_str!("getDimensionPixelSize"),
                jni_sig!("(I)I"),
                &[JValue::Int(id)],
            )?
            .i()?;
        Ok(px)
    })
    .map_err(Into::into)
}

/// winit 0.30 的安卓后端不匹配 `InputEvent::TextEvent`，输入法提交的文本到不了 egui。
/// GameActivity 把同一份文本留在 `text_input_state` 里，这里每帧取出来喂给 egui，
/// 取完清空，让那块缓冲只当输入法的暂存区。
/// 必须在 UI 线程的帧首调用：从别的线程推事件会和 egui 的 `end_pass` 清事件抢跑。
pub fn pump_ime(ctx: &egui::Context) {
    let Some(app) = APP.get() else { return };
    let state = app.text_input_state();
    if state.text.is_empty() || state.compose_region.is_some() {
        return;
    }
    let text = state.text;
    app.set_text_input_state(android_activity::input::TextInputState {
        text: String::new(),
        selection: android_activity::input::TextSpan { start: 0, end: 0 },
        compose_region: None,
    });
    log::debug!("输入法搬运: {text:?}");
    ctx.input_mut(|i| i.events.push(egui::Event::Text(text)));
}

/// 用户点了输入框，但 egui-winit 只在"要不要 IME"翻转时才发请求；
/// 键盘被返回键收掉后焦点没变，翻转不会发生，只能我们自己去要一次。
/// 参数 false = 不带 SHOW_IMPLICIT，等价于强制弹出（被用户主动收起后仍能要回）。
pub fn show_keyboard() {
    if let Some(app) = APP.get() {
        app.show_soft_input(false);
    }
}

pub fn toast(message: String) {
    let Some(app) = APP.get() else {
        log::error!("AndroidApp 未绑定，提示发不出去: {message}");
        return;
    };
    let worker = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        if let Err(e) = show_toast(&worker, &message) {
            log::error!("提示弹出失败: {e}");
        }
    }));
}

fn show_toast(app: &AndroidApp, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) };
    vm.attach_current_thread(|env| {
        let activity =
            unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) };
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
