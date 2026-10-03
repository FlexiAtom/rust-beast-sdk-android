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

use eframe::egui;

pub struct BeastApp {
    /// 唯一的文本框：人话和兽音在这里互相覆盖
    text: String,
    /// 勾选 = 完整串（头尾按当前字典的 4+2+1 / 3）；不勾 = 裸正文
    mainstream: bool,
    /// 字典输入框，翻译时现取，不合法就报错
    dict_input: String,
    /// 待弹出的「提示」
    notice: Option<String>,
}

impl Default for BeastApp {
    fn default() -> Self {
        Self {
            text: String::new(),
            mainstream: true,
            dict_input: beast::BEAST.iter().collect(),
            notice: None,
        }
    }
}

impl BeastApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        Self::default()
    }

    /// 人话 → 兽音：结果覆盖文本框。字典不合法则原样返回 Err，文本框不动。
    fn translate_to_beast(&mut self) -> Result<(), String> {
        let dict = Self::active_dict(&self.dict_input)?;
        self.text = if self.mainstream {
            dict.encode(&self.text)
        } else {
            dict.encode_body(&self.text)
        };
        Ok(())
    }

    /// 兽音 → 人话：解得动才覆盖文本框。
    fn translate_to_human(&mut self) -> Result<(), String> {
        let dict = Self::active_dict(&self.dict_input)?;
        let decoded = if self.mainstream {
            dict.decode(&self.text)
        } else {
            dict.decode_body(&self.text)
        };
        self.text = decoded.map_err(|e| e.to_string())?;
        Ok(())
    }

    fn active_dict(input: &str) -> Result<beast::BeastDict, String> {
        beast::BeastDict::parse(input).map_err(|e| e.to_string())
    }

    fn show_notice(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "android")]
        let _ = ctx;
        #[cfg(target_os = "android")]
        if let Some(message) = self.notice.take() {
            android::toast(message);
        }
        #[cfg(not(target_os = "android"))]
        if self.notice.is_some() {
            let mut closed = false;
            egui::Window::new("提示")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(self.notice.as_deref().unwrap_or_default());
                    closed = ui.button("关闭").clicked();
                });
            if closed {
                self.notice = None;
            }
        }
    }

    /// 点输入框就强制要一次软键盘。egui-winit 只在"要不要 IME"翻转时才发请求，
    /// 键盘被返回键收掉后焦点没变、不再翻转，光靠它要不回来。
    #[cfg(target_os = "android")]
    fn kick_keyboard(response: &egui::Response) {
        if response.has_focus() && (response.gained_focus() || response.clicked()) {
            android::show_keyboard();
        }
    }

    #[cfg(not(target_os = "android"))]
    fn kick_keyboard(_response: &egui::Response) {}
}

impl eframe::App for BeastApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "android")]
        android::pump_ime(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            #[cfg(target_os = "android")]
            ui.add_space(android::top_inset_px() as f32 / ui.ctx().pixels_per_point());
            // 第 1 行文本框，第 2~4 行是按键 / 字典 / 主流兼容，高度按行高预留
            let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
            let text_edit = ui.add(
                egui::TextEdit::multiline(&mut self.text)
                    .desired_width(f32::INFINITY)
                    .min_size(egui::vec2(
                        ui.available_width(),
                        (ui.available_height() - 3.0 * row).max(80.0),
                    )),
            );
            Self::kick_keyboard(&text_edit);
            ui.horizontal(|ui| {
                if ui.button("翻译为兽音").clicked() {
                    if let Err(e) = self.translate_to_beast() {
                        self.notice = Some(e);
                    }
                }
                if ui.button("翻译为人话").clicked() {
                    if let Err(e) = self.translate_to_human() {
                        self.notice = Some(e);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("字典");
                let dict_edit = ui.add(
                    egui::TextEdit::singleline(&mut self.dict_input)
                        .desired_width(120.0)
                        .hint_text("嗷呜啊~"),
                );
                Self::kick_keyboard(&dict_edit);
            });
            ui.checkbox(&mut self.mainstream, "主流兼容");
        });
        self.show_notice(ctx);
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let han = egui::FontData {
        font: std::borrow::Cow::Borrowed(include_bytes!("../assets/SourceHanSansCN-Regular.otf")),
        index: 0,
        tweak: egui::FontTweak::default(),
    };
    let han = std::sync::Arc::new(han);
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "source-han-sans-cn".to_owned());
    }
    fonts.font_data.insert("source-han-sans-cn".to_owned(), han);
    ctx.set_fonts(fonts);
}

pub fn start(app: eframe::NativeOptions) -> Result<(), eframe::Error> {
    eframe::run_native(
        "兽音译者",
        app,
        Box::new(|cc| Ok(Box::new(BeastApp::new(cc)))),
    )
}

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("beast-app"),
    );
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
    log::info!("android_main 进入");
    android::bind(&app);
    // eframe 0.31 要求把 AndroidApp 放在这里，它自己调 with_android_app；
    // 只挂 event_loop_builder 会被它忽略，报 "missing required android_app"
    let options = eframe::NativeOptions {
        android_app: Some(app),
        ..Default::default()
    };
    match start(options) {
        Ok(()) => log::info!("事件循环返回"),
        Err(e) => log::error!("eframe 失败: {e:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainstream_on_wraps_with_affix_and_overwrites_the_box() {
        let mut app = BeastApp {
            text: "你好".into(),
            ..Default::default()
        };
        app.translate_to_beast().unwrap();
        assert_eq!(app.text, beast::encode("你好"));
        app.translate_to_human().unwrap();
        assert_eq!(app.text, "你好");
    }

    #[test]
    fn mainstream_off_uses_bare_body() {
        let mut app = BeastApp {
            text: "你好".into(),
            mainstream: false,
            ..Default::default()
        };
        app.translate_to_beast().unwrap();
        assert_eq!(app.text, beast::encode_body("你好"));
        assert!(!app.text.starts_with(beast::AFFIX_HEAD));
        app.translate_to_human().unwrap();
        assert_eq!(app.text, "你好");
    }

    #[test]
    fn bad_dict_reports_and_leaves_the_box_untouched() {
        let mut app = BeastApp {
            text: "你好".into(),
            dict_input: "aaaa".into(),
            ..Default::default()
        };
        let err = app.translate_to_beast().unwrap_err();
        assert!(err.contains('重'), "{err}");
        assert_eq!(app.text, "你好");
    }

    #[test]
    fn undecodable_bare_body_reports_instead_of_silently_keeping_text() {
        let mut app = BeastApp {
            text: "呜嗷嗷".into(), // 奇数长度
            mainstream: false,
            ..Default::default()
        };
        let err = app.translate_to_human().unwrap_err();
        assert!(err.contains("2 的整数倍"), "{err}");
        assert_eq!(app.text, "呜嗷嗷");
    }

    #[test]
    fn custom_dict_end_to_end() {
        let mut app = BeastApp {
            text: "你好".into(),
            dict_input: "一二三四".into(),
            ..Default::default()
        };
        app.translate_to_beast().unwrap();
        assert!(
            app.text.starts_with("四二一") && app.text.ends_with('三'),
            "{:?}",
            app.text
        );
        app.translate_to_human().unwrap();
        assert_eq!(app.text, "你好");
    }

    #[test]
    fn dict_is_taken_at_translate_time() {
        // 默认字典编出来的串，换字典后应当解不动 —— 证明没有"应用"这一步的缓存
        let mut app = BeastApp {
            text: "你好".into(),
            ..Default::default()
        };
        app.translate_to_beast().unwrap();
        app.dict_input = "一二三四".into();
        assert!(app.translate_to_human().is_err());
    }
}
