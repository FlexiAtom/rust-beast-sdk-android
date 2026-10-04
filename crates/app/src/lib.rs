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

/// 长按菜单里可点的动作。选中范围由 egui 的 TextEdit 自己算（复制取那一段字符，
/// 走我们自己的剪贴板通路），粘贴递 `Event::Paste`，全选写它自己的选区状态。
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuPick {
    Copy,
    Paste,
    SelectAll,
}

/// 存在 `ctx.data` 里的那一格：手指压在屏上之前还在的非空选区，按 widget id 存。
#[derive(Clone, Copy)]
struct HeldSelection((usize, usize));

pub struct BeastApp {
    /// 唯一的文本框：人话和编码串在这里互相覆盖
    text: String,
    /// 勾选 = 完整串：编码时按当前字典加头尾，解码时**从串自己的头尾提字典**并回填字典框；
    /// 不勾 = 裸正文，两个方向都用字典框里那套
    mainstream: bool,
    /// 字典输入框，翻译时现取，不合法就报错
    dict_input: String,
    /// 待弹出的「提示」
    notice: Option<String>,
    /// 长按菜单点出来的动作，攒着**下一帧开头**喂进 egui 事件队列（现在只有粘贴）：
    /// `Event::Paste` 必须在 TextEdit 被绘制之前进队列，本帧内推会被 `end_pass` 清掉。
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

    /// 人话 → 编码串：结果覆盖文本框。字典不合法则原样返回 Err，文本框不动。
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
    ///
    /// 每帧在这里做一次选区记账：egui 在**按下那一帧**就把选区收成光标了
    /// （`text_cursor_state.rs` 里 `hovered() && any_pressed()` 那个分支），
    /// 而长按要等 `max_click_duration` 之后才出菜单，等到菜单时选区早没了——
    /// 所以"复制选中"必须靠这里记下的、手指还压在屏上之前那一份。
    fn text_menu(response: &egui::Response, out: &mut Option<(egui::Id, MenuPick)>) {
        let id = response.id;
        Self::remember_selection(&response.ctx, id);
        response.context_menu(move |ui| {
            Self::restore_selection(ui.ctx(), id);
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

    /// 非空选区的两端字符下标；光标（两端重合）按"没有选区"算。
    fn selection(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
        let range = egui::text_edit::TextEditState::load(ctx, id)?
            .cursor
            .char_range()?;
        let ends = range.sorted();
        (ends[0].index != ends[1].index).then_some((ends[0].index, ends[1].index))
    }

    /// 写 egui 自己那份选区状态，跟手指拖选后落的是同一个格子。
    /// 状态还没被 TextEdit 建出来时按默认值补一份：下一帧 TextEdit 会拿自己的
    /// galley 把区间钳回实际长度，不会越界。
    fn set_selection(ctx: &egui::Context, id: egui::Id, from: usize, to: usize) {
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text_selection::CCursorRange::two(
                egui::epaint::text::cursor::CCursor::new(from),
                egui::epaint::text::cursor::CCursor::new(to),
            )));
        state.store(ctx, id);
    }

    /// 手指不在屏上的那些帧，把当前选区存进 `ctx.data`；在屏上的帧跳过——
    /// 按下那一帧正是 egui 吃掉选区的那一帧，抬手那一帧存的才是拖选的最终结果。
    fn remember_selection(ctx: &egui::Context, id: egui::Id) {
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        // 先在外头读走：`TextEditState::load` 也要拿 data，写锁里再开读锁会自锁
        let selection = Self::selection(ctx, id);
        ctx.data_mut(|data| match selection {
            Some(range) => data.insert_temp(id, HeldSelection(range)),
            None => data.remove::<HeldSelection>(id),
        });
    }

    /// 菜单打开的每一帧调用：选区还在就什么都不做，被按下那一下吃了就补回去
    /// （补回来用户看得见高亮，也知道这条"复制"要复制的是哪一段）。
    fn restore_selection(ctx: &egui::Context, id: egui::Id) {
        let held = ctx.data(|data| data.get_temp::<HeldSelection>(id));
        if let Some(HeldSelection((from, to))) = held.filter(|_| Self::selection(ctx, id).is_none())
        {
            Self::set_selection(ctx, id, from, to);
        }
    }

    /// 这条「复制」该往系统剪贴板放什么：有选区就那一段，没选区就整个文本框。
    /// 不能投 `egui::Event::Copy` 让 egui 自己复制——见 `clipboard` 模块头：安卓上
    /// egui-winit 的剪贴板整段被 cfg 排掉，那份复制只活在本 app 内，粘到别处是空的。
    fn copy_payload(text: &str, selection: Option<(usize, usize)>) -> String {
        match selection {
            Some((from, to)) => text
                .chars()
                .skip(from)
                .take(to.saturating_sub(from))
                .collect(),
            None => text.to_owned(),
        }
    }

    /// 复制自己走 `clipboard`，粘贴与全选都递给 egui，由它在**下一帧**
    /// （`pending_events` 在帧首入队）按自己的规则替换选区或插入光标处。
    fn apply_menu(&mut self, ctx: &egui::Context, id: egui::Id, pick: MenuPick) {
        match pick {
            MenuPick::Copy => {
                let selection = Self::selection(ctx, id);
                let payload = Self::copy_payload(&self.text, selection);
                if payload.is_empty() {
                    self.notice = Some("文本框是空的，没东西可复制".to_owned());
                } else {
                    let chars = payload.chars().count();
                    let scope = if selection.is_some() {
                        "选中"
                    } else {
                        "全部"
                    };
                    clipboard::copy(&payload);
                    self.notice = Some(format!("已复制{scope} {chars} 个字"));
                }
            }
            MenuPick::Paste => match clipboard::paste() {
                Some(text) => self.pending_events.push(egui::Event::Paste(text)),
                None => self.notice = Some("系统剪贴板是空的（或取不到）".to_owned()),
            },
            MenuPick::SelectAll => {
                // egui 0.31 没有 `Event::SelectAll`，但它把选区存在公开的 `TextEditState` 里，
                // 写整段区间就是它自己的全选（高亮、之后的复制/粘贴都按这个选区走）。
                let len = self.text.chars().count();
                Self::set_selection(ctx, id, 0, len);
            }
        }
        ctx.request_repaint();
    }

    /// 控件排：两个按钮 + 字典框 + 主流兼容，全挤在同一行里，窄屏放不下时
    /// `horizontal_wrapped` 让它自己折到第二行——底栏随之长高，不会像原先那样
    /// 把最后一行留在屏幕圆角底下。抽成方法是为了让测试能量到真控件：
    /// `eframe::App::update` 需要 `Frame`，测试里构造不出来。
    fn controls(&mut self, ui: &mut egui::Ui, menu_pick: &mut Option<(egui::Id, MenuPick)>) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("编码").clicked() {
                if let Err(e) = self.translate_to_beast() {
                    self.notice = Some(e);
                }
            }
            if ui.button("解码").clicked() {
                if let Err(e) = self.translate_to_human() {
                    self.notice = Some(e);
                }
            }
            ui.label("字典");
            let dict_edit = ui.add(
                egui::TextEdit::singleline(&mut self.dict_input)
                    .desired_width(56.0)
                    .hint_text("嗷呜啊~"),
            );
            Self::kick_keyboard(&dict_edit);
            Self::text_menu(&dict_edit, menu_pick);
            ui.checkbox(&mut self.mainstream, "主流兼容");
        });
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
        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            self.controls(ui, &mut menu_pick);
            // 全屏 SurfaceView 会画到导航栏底下，不留这段就被屏幕底边和圆角切掉
            #[cfg(target_os = "android")]
            ui.add_space(android::bottom_inset_px() as f32 / ui.ctx().pixels_per_point());
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            #[cfg(target_os = "android")]
            ui.add_space(android::top_inset_px() as f32 / ui.ctx().pixels_per_point());
            // 文本框吃满剩下的整块高度，不再按"预留几行"估——控件行折不折都影响不到它。
            let size = ui.available_size();
            let text_edit = ui.add(
                egui::TextEdit::multiline(&mut self.text)
                    .desired_width(f32::INFINITY)
                    .min_size(size),
            );
            Self::kick_keyboard(&text_edit);
            Self::text_menu(&text_edit, &mut menu_pick);
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

    /// 读写选区一律转调实现里那两个函数，测试不再自己抄一份
    fn select(ctx: &egui::Context, id: egui::Id, from: usize, to: usize) {
        BeastApp::set_selection(ctx, id, from, to);
    }

    fn selected_range(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
        BeastApp::selection(ctx, id)
    }

    /// 有选区时复制的就是那一段，而且走我们自己的剪贴板通路——不能投 `Event::Copy`
    #[test]
    fn copy_with_selection_sends_just_that_slice() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("box");
        let mut app = BeastApp {
            text: "呜啊呜嗷".into(),
            ..Default::default()
        };
        select(&ctx, id, 1, 3);
        app.apply_menu(&ctx, id, MenuPick::Copy);
        assert!(
            app.pending_events.is_empty(),
            "复制不该再走 egui 的 Event::Copy：安卓上它到不了系统剪贴板"
        );
        let notice = app.notice.expect("复制选中也该给出字数");
        assert!(notice.contains("选中") && notice.contains('2'), "{notice}");
    }

    /// 选区是字符下标不是字节下标；旧选区越界也不能 panic
    #[test]
    fn copy_payload_slices_by_chars_not_bytes() {
        assert_eq!(BeastApp::copy_payload("呜啊呜嗷", Some((1, 3))), "啊呜");
        assert_eq!(BeastApp::copy_payload("呜啊呜嗷", None), "呜啊呜嗷");
        assert_eq!(
            BeastApp::copy_payload("呜啊", Some((5, 9))),
            "",
            "文本变短后的旧选区该切成空，而不是崩"
        );
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
        let notice = app.notice.expect("全选后复制也该给出字数");
        assert!(
            notice.contains("选中") && notice.contains('4'),
            "全选后该走选区那条路：{notice}"
        );
        assert!(app.pending_events.is_empty());
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

    /// 按给定宽度跑两帧取稳态，返回底栏那一块的落位。
    /// 两帧是必需的：egui 的面板按上一帧的落位预留空间，第一帧的尺寸还是旧值。
    fn settled_controls_rect(ctx: &egui::Context, app: &mut BeastApp, width: f32) -> egui::Rect {
        let mut rect = egui::Rect::NOTHING;
        for frame in 0..2 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 600.0),
                    )),
                    time: Some(1.0 + frame as f64 / 10.0),
                    ..Default::default()
                },
                |ctx| {
                    egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
                        let mut pick = None;
                        app.controls(ui, &mut pick);
                        rect = ui.max_rect();
                    });
                    egui::CentralPanel::default().show(ctx, |_| {});
                },
            );
        }
        rect
    }

    /// 控件排的适配假设：手机宽度放得下一行；放不下时底栏靠**长高**折行，
    /// 而不是把多出来的一截裁掉。`update()` 本身测不了（`eframe::Frame` 构造不出来），
    /// 这里量的是同一个 `controls()`。
    #[test]
    fn phone_width_keeps_one_row_and_tiny_width_grows_the_panel_upwards() {
        let ctx = egui::Context::default();
        let mut app = BeastApp::default();
        let roomy = settled_controls_rect(&ctx, &mut app, 2000.0);
        let phone = settled_controls_rect(&ctx, &mut app, 360.0);
        let tiny = settled_controls_rect(&ctx, &mut app, 120.0);

        assert!(
            (phone.height() - roomy.height()).abs() < 1.0,
            "360 宽就该是一行，别到了手机上才折：roomy={:?} phone={:?}",
            roomy,
            phone
        );
        assert!(
            tiny.height() > roomy.height() * 1.5,
            "120 宽放不下却没长高，说明它按裁剪处理：roomy={:?} tiny={:?}",
            roomy,
            tiny
        );
        // 长高必须是往上顶（底栏的底边由屏幕决定，动不了）。底边若跟着往下跑，
        // 就等于把多出来的一行推给了屏幕底边和圆角去切。
        assert!(
            (tiny.bottom() - phone.bottom()).abs() < 1.0,
            "底边不该移动：phone={:?} tiny={:?}",
            phone,
            tiny
        );
        assert!(
            tiny.min.y < phone.min.y - 1.0,
            "多出来的一行应当往屏幕中间长：phone={:?} tiny={:?}",
            phone,
            tiny
        );
    }

    /// 一帧里 egui 的实际状态：认没认出长按、菜单开没开、选区和记账还剩什么。
    #[derive(Default)]
    struct Frame {
        id: Option<egui::Id>,
        long_touched: bool,
        menu_open: bool,
        selection: Option<(usize, usize)>,
        held: Option<(usize, usize)>,
    }

    /// 跑一帧：画出主文本框并走一遍 `text_menu` 的记账，把这一帧的状态带回来。
    /// 只画这一个控件，所以 id 每帧都相同（第一帧拿它，后面几帧再用）。
    /// 选区、记账、菜单都读在 `text_menu` **之后**——闭包里补回来的那一份才算数。
    fn frame_text_edit(
        ctx: &egui::Context,
        text: &mut String,
        events: Vec<egui::Event>,
        pointer_pos: Option<egui::Pos2>,
        time: f64,
    ) -> Frame {
        let mut frame = Frame::default();
        // 先报一次手指在哪，再报按下/抬起——`Response::hovered()` 要的是这一帧的指针位置
        let mut all = Vec::new();
        if let Some(pos) = pointer_pos {
            all.push(egui::Event::PointerMoved(pos));
        }
        all.extend(events);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360.0, 600.0),
                )),
                time: Some(time),
                events: all,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(text)
                            .desired_width(f32::INFINITY)
                            .min_size(ui.available_size()),
                    );
                    let id = response.id;
                    frame.id = Some(id);
                    frame.long_touched = response.long_touched();
                    let mut pick = None;
                    BeastApp::text_menu(&response, &mut pick);
                    frame.menu_open = response.context_menu_opened();
                    frame.selection = BeastApp::selection(ctx, id);
                    frame.held = ctx
                        .data(|data| data.get_temp::<HeldSelection>(id))
                        .map(|held| held.0);
                });
            },
        );
        frame
    }

    /// 手指按在文本框中间，等价于触屏长按的那一下按下
    const PRESS: egui::Pos2 = egui::Pos2::new(80.0, 80.0);

    fn press_event(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    /// 选区 + 一帧按下：egui 当场把选区收成光标（这就是人工报的那个 bug），
    /// 但记账要活下来——菜单靠它才有东西可复制。
    #[test]
    fn the_press_eats_the_selection_but_the_bookkeeping_survives() {
        let ctx = egui::Context::default();
        let mut text = "呜啊呜嗷".to_owned();
        let id = frame_text_edit(&ctx, &mut text, vec![], None, 1.0)
            .id
            .unwrap();
        select(&ctx, id, 0, 2);
        frame_text_edit(&ctx, &mut text, vec![], None, 2.0);
        assert_eq!(selected_range(&ctx, id), Some((0, 2)), "抬手帧之后选区还在");
        assert_eq!(held_selection(&ctx, id), Some((0, 2)));

        let frame = frame_text_edit(
            &ctx,
            &mut text,
            vec![press_event(PRESS, true)],
            Some(PRESS),
            3.0,
        );
        assert_eq!(
            frame.selection, None,
            "按下这一帧 egui 该把选区收成光标——它不收成这样就没有这个 bug 了"
        );
        assert_eq!(frame.held, Some((0, 2)), "记账不能被这一帧冲掉");
    }

    /// 菜单打开的那一帧把选区补回来，之后"复制"走的才是选区那条路（不是整框兜底）。
    #[test]
    fn the_menu_puts_the_selection_back_so_copy_uses_it() {
        let ctx = egui::Context::default();
        let mut text = "呜啊呜嗷".to_owned();
        let id = frame_text_edit(&ctx, &mut text, vec![], None, 1.0)
            .id
            .unwrap();
        select(&ctx, id, 1, 3);
        frame_text_edit(&ctx, &mut text, vec![], None, 2.0);
        frame_text_edit(
            &ctx,
            &mut text,
            vec![press_event(PRESS, true)],
            Some(PRESS),
            3.0,
        );
        assert_eq!(selected_range(&ctx, id), None);

        BeastApp::restore_selection(&ctx, id);
        assert_eq!(
            selected_range(&ctx, id),
            Some((1, 3)),
            "菜单里该重新看到高亮"
        );

        let mut app = BeastApp {
            text: text.clone(),
            ..Default::default()
        };
        app.apply_menu(&ctx, id, MenuPick::Copy);
        let notice = app.notice.expect("补回来的选区该给出字数");
        assert!(
            notice.contains("选中") && notice.contains('2'),
            "补回来的选区该走选区那条路，不是整框兜底：{notice}"
        );
        assert!(app.pending_events.is_empty());
    }

    /// 抬手后点一下别处（选区被用户自己取消）时，记账必须跟着清掉，
    /// 否则下一次长按会复制一段早就不算数的旧选区。
    #[test]
    fn a_tap_that_drops_the_selection_also_drops_the_bookkeeping() {
        let ctx = egui::Context::default();
        let mut text = "呜啊呜嗷".to_owned();
        let id = frame_text_edit(&ctx, &mut text, vec![], None, 1.0)
            .id
            .unwrap();
        select(&ctx, id, 0, 2);
        frame_text_edit(&ctx, &mut text, vec![], None, 2.0);
        assert_eq!(held_selection(&ctx, id), Some((0, 2)));

        // 按下 → 抬手，等价于一次单击：egui 把光标落到点处，选区没了
        frame_text_edit(
            &ctx,
            &mut text,
            vec![press_event(PRESS, true)],
            Some(PRESS),
            3.0,
        );
        frame_text_edit(
            &ctx,
            &mut text,
            vec![press_event(PRESS, false)],
            Some(PRESS),
            4.0,
        );
        assert_eq!(selected_range(&ctx, id), None);
        assert_eq!(
            held_selection(&ctx, id),
            None,
            "用户自己取消的选区不该被记着，下次长按不能拿它复制"
        );
    }

    fn held_selection(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
        ctx.data(|data| data.get_temp::<HeldSelection>(id))
            .map(|held| held.0)
    }

    /// 触屏的那一下按下：GameActivity 会同时给 `Touch` 和 `PointerButton`，
    /// egui 的 `is_long_touch()` 要的是前者（`any_touches()` 只由 `Event::Touch` 喂）。
    fn touch_start(pos: egui::Pos2) -> egui::Event {
        egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(0),
            phase: egui::TouchPhase::Start,
            pos,
            force: None,
        }
    }

    /// 真按 egui 的长按通路走一遍：选区先被按下吃掉 → 手指不落下去、保持超过
    /// `max_click_duration` → egui 自己把它翻成 secondary click → 菜单打开。
    /// 补选区只写在 `context_menu` 的闭包里，所以"菜单开着 + 选区回来了"
    /// 就等于那段闭包真的跑过——不用去够 egui 私有的菜单状态。
    #[test]
    fn a_real_long_press_opens_the_menu_with_the_selection_back() {
        let ctx = egui::Context::default();
        let mut text = "呜啊呜嗷".to_owned();
        let id = frame_text_edit(&ctx, &mut text, vec![], None, 1.0)
            .id
            .unwrap();
        select(&ctx, id, 1, 3);
        frame_text_edit(&ctx, &mut text, vec![], None, 2.0);

        let frame = frame_text_edit(
            &ctx,
            &mut text,
            vec![press_event(PRESS, true)],
            Some(PRESS),
            3.0,
        );
        assert_eq!(frame.selection, None, "按下这一帧选区被 egui 收成光标");
        assert_eq!(frame.held, Some((1, 3)), "手指还压着，记账该留着");

        // 手指没动也没抬，只是过了一秒，并且这一下是触屏
        let frame = frame_text_edit(&ctx, &mut text, vec![touch_start(PRESS)], Some(PRESS), 4.0);
        assert!(frame.long_touched, "egui 该把这一帧认成触屏长按");
        assert_eq!(
            frame.selection,
            Some((1, 3)),
            "菜单没开、或开了却没把选区补回来"
        );

        let frame = frame_text_edit(&ctx, &mut text, vec![], Some(PRESS), 5.0);
        assert!(frame.menu_open, "长按之后菜单该开着");
        assert_eq!(frame.selection, Some((1, 3)), "菜单开着的每一帧选区都还在");
    }
}
