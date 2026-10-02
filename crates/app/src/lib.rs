use eframe::egui;

pub struct BeastApp {
    /// 人话文本框
    human: String,
    /// 兽音文本框
    beast_text: String,
    /// 字典编辑区（应用成功后才写进 dict）
    dict_input: String,
    dict: beast::BeastDict,
    status: String,
}

impl Default for BeastApp {
    fn default() -> Self {
        Self {
            human: String::new(),
            beast_text: String::new(),
            dict_input: beast::BEAST.iter().collect(),
            dict: beast::BeastDict::DEFAULT,
            status: String::new(),
        }
    }
}

impl BeastApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        Self::default()
    }

    /// 人话 → 兽音：结果覆盖兽音文本框
    fn translate_to_beast(&mut self) {
        self.beast_text = self.dict.encode(&self.human);
        self.status = format!("已写入兽音框，{} 字", self.beast_text.chars().count());
    }

    /// 兽音 → 人话：结果覆盖人话文本框；失败不动人话框
    fn translate_to_human(&mut self) {
        match self.dict.decode(&self.beast_text) {
            Ok(text) => {
                self.status = format!("已写入人话框，{} 字", text.chars().count());
                self.human = text;
            }
            Err(e) => {
                self.status = format!("失败：{e}");
            }
        }
    }

    /// 应用字典编辑区的内容；成功后编辑区规范化为当前字典
    fn apply_dict(&mut self) {
        match beast::BeastDict::parse(&self.dict_input) {
            Ok(dict) => {
                self.dict = dict;
                self.dict_input = dict.chars().iter().collect();
                self.status = format!("字典已生效：{:?}，头尾={:?}", dict.chars(), self.dict.head());
            }
            Err(e) => {
                self.status = format!("字典无效：{e}");
            }
        }
    }
}

impl eframe::App for BeastApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::CollapsingHeader::new("兽音字典（默认 嗷呜啊~，4 个互不重复的字符）")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.dict_input)
                                .desired_width(120.0)
                                .hint_text("嗷呜啊~"),
                        );
                        if ui.button("应用").clicked() {
                            self.apply_dict();
                        }
                        if ui.button("恢复默认").clicked() {
                            self.dict_input = beast::BEAST.iter().collect();
                            self.apply_dict();
                        }
                    });
                });
            ui.label("文本框");
            let box_height = (ui.available_height() - 40.0) * 0.5;
            ui.add(
                egui::TextEdit::multiline(&mut self.human)
                    .desired_width(f32::INFINITY)
                    .min_size(egui::vec2(ui.available_width(), box_height)),
            );
            ui.horizontal(|ui| {
                if ui.button("翻译为兽音").clicked() {
                    self.translate_to_beast();
                }
                if ui.button("翻译为人话").clicked() {
                    self.translate_to_human();
                }
                ui.label("兽音文本框");
                let width = ui.available_width();
                ui.add(
                    egui::TextEdit::multiline(&mut self.beast_text)
                        .min_size(egui::vec2(width, box_height)),
                );
            });
            ui.label(&self.status);
        });
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let wqy = egui::FontData {
        font: std::borrow::Cow::Borrowed(include_bytes!("../assets/wqy-zenhei.ttc")),
        index: 0,
        tweak: egui::FontTweak::default(),
    };
    let wqy = std::sync::Arc::new(wqy);
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "wqy-zenhei".to_owned());
    }
    fonts.font_data.insert("wqy-zenhei".to_owned(), wqy);
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
#[no_mangle]
pub fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    let mut options = eframe::NativeOptions::default();
    options.event_loop_builder = Some(Box::new(move |builder| {
        builder.with_android_app(app.clone());
    }));
    let _ = start(options);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_beast_overwrites_beast_box() {
        let mut app = BeastApp { human: "你好".into(), ..Default::default() };
        app.translate_to_beast();
        assert_eq!(app.beast_text, beast::encode("你好"));
    }

    #[test]
    fn to_human_overwrites_human_box_on_round_trip() {
        let mut app = BeastApp { human: "a\tb😀".into(), ..Default::default() };
        app.translate_to_beast();
        app.human.clear();
        app.translate_to_human();
        assert_eq!(app.human, "a\tb😀");
        assert!(app.status.starts_with("已写入人话框"));
    }

    #[test]
    fn to_human_failure_keeps_human_box() {
        let mut app = BeastApp {
            human: "别覆盖我".into(),
            beast_text: "呜嗷嗷".into(), // 缺头尾
            ..Default::default()
        };
        app.translate_to_human();
        assert_eq!(app.human, "别覆盖我");
        assert!(app.status.contains("失败"));
    }

    #[test]
    fn custom_dict_end_to_end() {
        let mut app = BeastApp { dict_input: "一二三四".into(), ..Default::default() };
        app.apply_dict();
        assert!(app.status.starts_with("字典已生效"));
        app.human = "你好".into();
        app.translate_to_beast();
        assert!(app.beast_text.starts_with("四二一"));
        assert!(app.beast_text.ends_with('三'));
        app.human.clear();
        app.translate_to_human();
        assert_eq!(app.human, "你好");
        // 默认字典解不了自定义字典的串
        app.dict_input = "嗷呜啊~".into();
        app.apply_dict();
        app.translate_to_human();
        assert!(app.status.contains("失败"));
    }

    #[test]
    fn invalid_dict_rejected_and_active_dict_untouched() {
        let mut app = BeastApp { dict_input: "aaaa".into(), ..Default::default() };
        app.apply_dict();
        assert!(app.status.contains("字典无效"));
        assert_eq!(app.dict, beast::BeastDict::DEFAULT);
    }
}
