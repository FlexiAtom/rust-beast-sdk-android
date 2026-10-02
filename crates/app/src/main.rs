fn main() {
    let options = eframe::NativeOptions::default();
    if beast_app::start(options).is_err() {
        std::process::exit(1);
    }
}
