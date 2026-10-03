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

/// 长按菜单里可点的动作。选中范围、复制内容都由 egui 的 TextEdit 自己算，
/// 这里只是把它的 `Event::Copy` / `Event::Paste` 递进队列，全选写它自己的选区状态。
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuPick {
    Copy,
    Paste,
    SelectAll,
}

pub struct BeastApp {
    /// 唯一的文本框：人话和兽音在这里互相覆盖
    text: String,
    /// 勾选 = 完整串：编码时按当前字典加头尾，解码时**从串自己的头尾提字典**并回填字典框；
    /// 不勾 = 裸正文，两个方向都用字典框里那套
    mainstream: bool,
    /// 字典输入框，翻译时现取，不合法就报错
    dict_input: String,
    /// 待弹出的「提示」
    notice: Option<String>,
    /// 长按菜单点出来的动作，攒着**下一帧开头**喂进 egui 事件队列：`Event::Copy/Paste`
    /// 必须在 TextEdit 被绘制之前进队列，本帧内推会被 `end_pass` 清掉。
    pending_events: Vec<egui::Event>,
}

impl Default for BeastApp {
    fn default() -> Self {
        Self {
            text: String::new(),
            mainstream: true,
            dict_input: beast::BEAST.iter().collect(),
            notice: None,
            pending_events: Vec::new(),
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
    ///
    /// 勾了主流兼容时，字典**从串自己的头尾里提取**（头 = 4+2+1、尾 = 3，四个位置正好覆盖整套
    /// 字典一次），并把提出来的那套回填进字典框——否则下一步反向翻译会用错字典。字典框里的值在这
    /// 条路上只是被更新，不是前提条件。
    ///
    /// 末尾凑不满一个字的残缺字符按主流行为静默丢弃，但这里给出一条提示，避免用户以为解全了。
    fn translate_to_human(&mut self) -> Result<(), String> {
        let (decoded, dropped) = if self.mainstream {
            let (text, dict, dropped) =
                beast::decode_mainstream(&self.text).map_err(|e| e.to_string())?;
            self.dict_input = dict.chars().into_iter().collect();
            (text, dropped)
        } else {
            let dict = Self::active_dict(&self.dict_input)?;
            dict.decode_body_with_tail(&self.text)
                .map_err(|e| e.to_string())?
        };
        self.text = decoded;
        if dropped > 0 {
            self.notice = Some(format!(
                "末尾 {dropped} 个字符凑不满一个字，已按主流行为丢弃"
            ));
        }
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

    /// 长按（egui 把触屏长按转成 secondary click）弹它自带的 `context_menu`。
    fn text_menu(response: &egui::Response, out: &mut Option<(egui::Id, MenuPick)>) {
        let id = response.id;
        response.context_menu(|ui| {
            if ui.button("复制").clicked() {
                *out = Some((id, MenuPick::Copy));
                ui.close_menu();
            }
            if ui.button("粘贴").clicked() {
                *out = Some((id, MenuPick::Paste));
                ui.close_menu();
            }
            if ui.button("全选").clicked() {
                *out = Some((id, MenuPick::SelectAll));
                ui.close_menu();
            }
        });
    }

    /// 复制走 egui 的选区；没有选区时复制整个文本框。粘贴与全选都递给 egui，
    /// 由它在**下一帧**（`pending_events` 在帧首入队）按自己的规则替换选区或插入光标处。
    fn apply_menu(&mut self, ctx: &egui::Context, id: egui::Id, pick: MenuPick) {
        let selected = egui::text_edit::TextEditState::load(ctx, id)
            .and_then(|state| state.cursor.char_range())
            .is_some_and(|range| {
                let ends = range.sorted();
                ends[0].index != ends[1].index
            });
        match pick {
            MenuPick::Copy if selected => self.pending_events.push(egui::Event::Copy),
            MenuPick::Copy => {
                if self.text.is_empty() {
                    self.notice = Some("文本框是空的，没东西可复制".to_owned());
                } else {
                    let chars = self.text.chars().count();
                    clipboard::copy(&self.text);
                    self.notice = Some(format!("已复制全部 {chars} 个字"));
                }
            }
            MenuPick::Paste => match clipboard::paste() {
                Some(text) => self.pending_events.push(egui::Event::Paste(text)),
                None => self.notice = Some("系统剪贴板是空的（或取不到）".to_owned()),
            },
            MenuPick::SelectAll => {
                // egui 0.31 没有 `Event::SelectAll`，但它把选区存在公开的 `TextEditState` 里，
                // 写整段区间就是它自己的全选（高亮、之后的复制/粘贴都按这个选区走）。
                // 状态还没被 TextEdit 建出来时按默认值补一份：下一帧 TextEdit 会拿自己的
                // galley 把区间钳回实际长度，不会越界。
                let len = self.text.chars().count();
                let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
                state
                    .cursor
                    .set_char_range(Some(egui::text_selection::CCursorRange::two(
                        egui::epaint::text::cursor::CCursor::new(0),
                        egui::epaint::text::cursor::CCursor::new(len),
                    )));
                state.store(ctx, id);
            }
        }
        ctx.request_repaint();
    }
}

impl eframe::App for BeastApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "android")]
        android::pump_ime(ctx);
        if !self.pending_events.is_empty() {
            let queued = std::mem::take(&mut self.pending_events);
            ctx.input_mut(|i| i.events.extend(queued));
        }
        let mut menu_pick: Option<(egui::Id, MenuPick)> = None;
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
            Self::text_menu(&text_edit, &mut menu_pick);
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
                Self::text_menu(&dict_edit, &mut menu_pick);
            });
            ui.checkbox(&mut self.mainstream, "主流兼容");
        });
        if let Some((id, pick)) = menu_pick.take() {
            self.apply_menu(ctx, id, pick);
        }
        // egui 的复制出口在安卓上是断的：egui-winit 0.31 的 clipboard.rs 把 arboard 整段挂在
        // `not(target_os = "android")` 上，于是 `OutputCommand::CopyText` 只落进它那个
        // "同一个 app 内才看得见"的兜底 `String`。这里在 egui-winit 之前把 CopyText 抢出来
        // 接到系统剪贴板，别的 app 才粘得出来；其余命令原样留着给它处理。桌面由 eframe 自己走。
        #[cfg(target_os = "android")]
        {
            let commands = ctx.output_mut(|o| std::mem::take(&mut o.commands));
            let mut keep = Vec::new();
            for command in commands {
                match command {
                    egui::OutputCommand::CopyText(text) => clipboard::copy(&text),
                    other => keep.push(other),
                }
            }
            if !keep.is_empty() {
                ctx.output_mut(|o| o.commands.extend(keep));
            }
        }
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

mod clipboard;

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

    /// 尾部残缺：照主流丢弃，但给出提示，不能无声解出半句
    #[test]
    fn incomplete_tail_decodes_but_warns() {
        let mut app = BeastApp {
            text: beast::encode_body("你好").chars().take(14).collect(),
            mainstream: false,
            ..Default::default()
        };
        app.translate_to_human().unwrap();
        assert_eq!(app.text, "你");
        let notice = app.notice.expect("尾部残缺应当提示");
        assert!(notice.contains('6'), "{notice}");
        assert!(notice.contains("丢弃"), "{notice}");
    }

    #[test]
    fn complete_decode_leaves_no_notice() {
        let mut app = BeastApp {
            text: beast::encode("你好"),
            ..Default::default()
        };
        app.translate_to_human().unwrap();
        assert_eq!(app.text, "你好");
        assert!(app.notice.is_none());
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
        // 裸正文不带字典，所以字典框改了就解不动——证明没有"应用"这一步的缓存
        let mut app = BeastApp {
            text: "你好".into(),
            mainstream: false,
            ..Default::default()
        };
        app.translate_to_beast().unwrap();
        app.dict_input = "一二三四".into();
        assert!(app.translate_to_human().is_err());
    }

    /// 勾了主流兼容时，字典是从串里提出来的，字典框填错也照样解得开
    #[test]
    fn mainstream_decode_reads_the_dictionary_out_of_the_string() {
        let dict = beast::BeastDict::new(['龙', '蛇', '龟', '鱼']).unwrap();
        let mut app = BeastApp {
            text: dict.encode("你好"),
            dict_input: "嗷呜啊~".into(), // 故意留一套不相干的字典
            ..Default::default()
        };
        app.translate_to_human().unwrap();
        assert_eq!(app.text, "你好");
        assert_eq!(app.dict_input, "龙蛇龟鱼", "应当把串自带的字典回填进框");
    }

    /// 主流串自带字典 ⇒ 反向翻译用的是同一套，来回一次得到原串
    #[test]
    fn mainstream_round_trip_survives_a_foreign_string() {
        let incoming = beast::BeastDict::parse("αβγδ").unwrap().encode("兽音");
        let mut app = BeastApp {
            text: incoming.clone(),
            ..Default::default()
        };
        app.translate_to_human().unwrap();
        assert_eq!(app.text, "兽音");
        app.translate_to_beast().unwrap();
        assert_eq!(app.text, incoming, "解出来再编回去应当逐字符相同");
    }

    /// 直接写 egui 自己那份选区状态，跟手指拖选后落的是同一个格子
    fn select(ctx: &egui::Context, id: egui::Id, from: usize, to: usize) {
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text_selection::CCursorRange::two(
                egui::epaint::text::cursor::CCursor::new(from),
                egui::epaint::text::cursor::CCursor::new(to),
            )));
        state.store(ctx, id);
    }

    fn selected_range(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
        egui::text_edit::TextEditState::load(ctx, id)
            .and_then(|state| state.cursor.char_range())
            .map(|range| {
                let ends = range.sorted();
                (ends[0].index, ends[1].index)
            })
    }

    /// 有选区时把 egui 自己的 `Event::Copy` 攒进队列，不碰文本、不弹提示
    #[test]
    fn copy_with_selection_queues_eguis_own_copy_event() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("box");
        let mut app = BeastApp {
            text: "呜啊呜嗷".into(),
            ..Default::default()
        };
        select(&ctx, id, 0, 2);
        app.apply_menu(&ctx, id, MenuPick::Copy);
        assert!(
            matches!(app.pending_events.as_slice(), [egui::Event::Copy]),
            "{:?}",
            app.pending_events
        );
        assert!(app.notice.is_none());
    }

    /// 没选区就复制整框——这是"复制文本里的内容"那条要求，不依赖手指能不能拖出选区
    #[test]
    fn copy_without_selection_copies_the_whole_box() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("box");
        let mut app = BeastApp {
            text: "呜啊呜嗷".into(),
            ..Default::default()
        };
        app.apply_menu(&ctx, id, MenuPick::Copy);
        assert!(app.pending_events.is_empty(), "没选区不该发 Event::Copy");
        let notice = app.notice.expect("复制全部应当给出字数");
        assert!(notice.contains('4'), "{notice}");
    }

    #[test]
    fn copy_of_empty_text_says_so_instead_of_claiming_a_copy() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("box");
        let mut app = BeastApp::default();
        app.apply_menu(&ctx, id, MenuPick::Copy);
        assert!(app.pending_events.is_empty());
        assert!(app.notice.expect("空框要有提示").contains("空"));
    }

    /// 全选 = 写满 egui 的选区，之后点复制走的是选区那条路（不是整框兜底）
    #[test]
    fn select_all_then_copy_goes_through_the_selection_path() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("box");
        let mut app = BeastApp {
            text: "呜啊呜嗷".into(),
            ..Default::default()
        };
        app.apply_menu(&ctx, id, MenuPick::SelectAll);
        assert_eq!(selected_range(&ctx, id), Some((0, 4)));
        app.apply_menu(&ctx, id, MenuPick::Copy);
        assert!(
            matches!(app.pending_events.as_slice(), [egui::Event::Copy]),
            "{:?}",
            app.pending_events
        );
        assert!(app.notice.is_none(), "全选后不该掉进\"已复制全部\"兜底");
    }

    /// 粘贴不能静默：要么把 Event::Paste 攒进队列，要么说明取不到
    #[test]
    fn paste_queues_an_event_or_says_why_it_did_not() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("box");
        let mut app = BeastApp::default();
        app.apply_menu(&ctx, id, MenuPick::Paste);
        assert!(
            !app.pending_events.is_empty() || app.notice.is_some(),
            "粘不动又不吭声，用户只会以为按钮坏了"
        );
        for event in &app.pending_events {
            if let egui::Event::Paste(text) = event {
                assert!(!text.is_empty(), "空剪贴板不该发 Paste 事件");
            }
        }
    }
}
