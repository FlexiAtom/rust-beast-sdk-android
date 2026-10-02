use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Encode,
    Decode,
}

pub struct BeastApp {
    input: String,
    output: String,
    mode: Mode,
    /// true = 裸正文入口：不附加也不剥除主流头尾（`~呜嗷` / `啊`）
    bare: bool,
    status: String,
}

impl Default for BeastApp {
    fn default() -> Self {
        Self {
            input: String::new(),
            output: String::new(),
            mode: Mode::Encode,
            bare: false,
            status: String::new(),
        }
    }
}

impl BeastApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        Self::default()
    }

    fn translate(&mut self) {
        let result = match (self.mode, self.bare) {
            (Mode::Encode, false) => Ok(beast::encode(&self.input)),
            (Mode::Encode, true) => Ok(beast::encode_body(&self.input)),
            (Mode::Decode, false) => beast::decode(&self.input),
            (Mode::Decode, true) => beast::decode_body(&self.input),
        };
        match result {
            Ok(text) => {
                self.status = format!("完成，{} 字符", text.chars().count());
                self.output = text;
            }
            Err(e) => {
                self.status = format!("失败：{e}");
            }
        }
    }
}

impl eframe::App for BeastApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.mode, Mode::Encode, "人话 → 兽语");
                ui.selectable_value(&mut self.mode, Mode::Decode, "兽语 → 人话");
            });
            ui.checkbox(
                &mut self.bare,
                "按裸正文处理（不附加/不剥除头尾 ～呜嗷…啊）",
            );
            ui.label("输入");
            let height = ui.available_height() * 0.42;
            ui.add(
                egui::TextEdit::multiline(&mut self.input)
                    .desired_rows(6)
                    .desired_width(f32::INFINITY)
                    .min_size(egui::vec2(ui.available_width(), height)),
            );
            ui.horizontal(|ui| {
                if ui.button("翻译").clicked() {
                    self.translate();
                }
                if ui.button("输出 ↔ 输入").clicked() {
                    std::mem::swap(&mut self.input, &mut self.output);
                    self.mode = match self.mode {
                        Mode::Encode => Mode::Decode,
                        Mode::Decode => Mode::Encode,
                    };
                }
                if ui.button("复制结果").clicked() && !self.output.is_empty() {
                    ctx.copy_text(self.output.clone());
                    self.status = "已复制到剪贴板".to_owned();
                }
                if ui.button("清空").clicked() {
                    self.input.clear();
                    self.output.clear();
                    self.status.clear();
                }
            });
            ui.label("输出");
            ui.add(
                egui::TextEdit::multiline(&mut self.output)
                    .desired_rows(6)
                    .desired_width(f32::INFINITY)
                    .interactive(false)
                    .min_size(egui::vec2(ui.available_width(), height)),
            );
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
