mod app;
mod bee_import;
mod board;
mod clipboard;
mod items;
mod persistence;
mod recent;
mod util;

use app::HyprBoardApp;

fn main() -> eframe::Result {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HyprBoard")
            .with_inner_size([1280.0, 720.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "HyprBoard",
        options,
        Box::new(|_cc| Ok(Box::new(HyprBoardApp::default()))),
    )
}
