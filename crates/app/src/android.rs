// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Copyright (C) 2026 FlexiAtom
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU Affero General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option) any
// later version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
// PARTICULAR PURPOSE. See the GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License along
// with this program. If not, see <https://www.gnu.org/licenses/>.

//! 安卓平台侧：原生「提示」（Toast）+ 把 GameActivity 的输入法缓冲搬进 egui + 系统剪贴板。

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::sync::OnceLock;

use eframe::egui;
use jni::objects::{JObject, JString, JValue};
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

/// 系统剪贴板写入。`ClipboardManager` 按 Android 的约定要在 Java 主线程用，
/// 所以和 Toast 一样走 `run_on_java_main_thread`。
pub fn clipboard_set(text: &str) {
    let Some(app) = APP.get() else {
        log::error!("AndroidApp 未绑定，复制发不出去");
        return;
    };
    let worker = app.clone();
    let owned = text.to_owned();
    app.run_on_java_main_thread(Box::new(move || {
        if let Err(e) = clipboard_write(&worker, &owned) {
            log::error!("写入系统剪贴板失败: {e}");
        }
    }));
}

/// 系统剪贴板读取。主线程那边排不上队（Activity 正在销毁等）就按取不到处理，
/// 不把 UI 线程挂住。
pub fn clipboard_get() -> Option<String> {
    let app = APP.get()?;
    let (tx, rx) = mpsc::channel();
    let worker = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        let _ = tx.send(clipboard_read(&worker));
    }));
    rx.recv_timeout(std::time::Duration::from_millis(500))
        .ok()
        .flatten()
}

/// `ClipboardManager` 的取用。参数类型必须是 `jni::Env`（带方法的那个）：
/// 0.22 起 `jni::JNIEnv` 是 `EnvUnowned` 的别名，只剩指针、没有 `new_string`/`call_method`。
fn clipboard_manager<'local>(
    env: &mut jni::Env<'local>,
    activity: &JObject<'local>,
) -> jni::errors::Result<JObject<'local>> {
    let name: JObject = env.new_string("clipboard")?.into();
    env.call_method(
        activity,
        jni_str!("getSystemService"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(&name)],
    )?
    .l()
}

fn clipboard_write(app: &AndroidApp, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) };
    // 闭包的错误类型要写死：`From<jni::Error>` 现在有好几个候选实现，`Ok(())` 推不出 `E`
    vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let activity =
            unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) };
        let manager = clipboard_manager(env, &activity)?;
        let label: JObject = env.new_string("兽音译者")?.into();
        let body: JObject = env.new_string(text)?.into();
        let clip = env.call_static_method(
            jni_str!("android/content/ClipData"),
            jni_str!("newPlainText"),
            jni_sig!(
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"
            ),
            &[JValue::Object(&label), JValue::Object(&body)],
        )?;
        let clip = clip.l()?;
        env.call_method(
            &manager,
            jni_str!("setPrimaryClip"),
            jni_sig!("(Landroid/content/ClipData;)V"),
            &[JValue::Object(&clip)],
        )?;
        Ok(())
    })
    .map_err(Into::into)
}

fn clipboard_read(app: &AndroidApp) -> Option<String> {
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) };
    vm.attach_current_thread(|env| -> jni::errors::Result<Option<String>> {
        let activity =
            unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) };
        let manager = clipboard_manager(env, &activity)?;
        let clip = env
            .call_method(
                &manager,
                jni_str!("getPrimaryClip"),
                jni_sig!("()Landroid/content/ClipData;"),
                &[],
            )?
            .l()?;
        if clip.is_null() {
            return Ok(None);
        }
        let count = env
            .call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?
            .i()?;
        if count <= 0 {
            return Ok(None);
        }
        let item = env
            .call_method(
                &clip,
                jni_str!("getItemAt"),
                jni_sig!("(I)Landroid/content/ClipData$Item;"),
                &[JValue::Int(0)],
            )?
            .l()?;
        let text = env
            .call_method(
                &item,
                jni_str!("coerceToText"),
                jni_sig!("(Landroid/content/Context;)Ljava/lang/CharSequence;"),
                &[JValue::Object(&activity)],
            )?
            .l()?;
        let text = env
            .call_method(
                &text,
                jni_str!("toString"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        // toString 已经保证是 java.lang.String，但 `.l()` 只给 JObject；
        // get_string 要 `AsRef<JString>`，所以按 jni 0.22 的 cast_local 转一次
        let text = env.cast_local::<JString>(text)?;
        let owned = text.try_to_string(env)?;
        Ok(if owned.is_empty() { None } else { Some(owned) })
    })
    .ok()
    .flatten()
}
