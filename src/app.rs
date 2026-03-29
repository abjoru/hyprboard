use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use crate::board::Board;
use crate::board::render::SELECTION_COLOR;
use crate::pdf_export::{PageSize, PdfMode};
use crate::persistence;
use crate::recent::RecentFiles;
use eframe::Frame;
use egui::{CentralPanel, Color32, Context, Key, TopBottomPanel, Vec2};

#[derive(Default, PartialEq)]
enum PendingAction {
    #[default]
    None,
    Open,
    Quit,
}

pub struct HyprBoardApp {
    board: Board,
    context_menu_pos: Option<egui::Pos2>,
    current_file: Option<PathBuf>,
    last_change: Option<Instant>,
    pending_action: PendingAction,
    show_unsaved_dialog: bool,
    recent_files: RecentFiles,
    show_toolbar: bool,
    last_title: String,
    show_pdf_dialog: bool,
    pdf_mode: PdfMode,
    pdf_page_size: PageSize,
    pending_pdf: Option<Vec<u8>>,
    pdf_tx: mpsc::Sender<Result<Vec<u8>, String>>,
    pdf_rx: mpsc::Receiver<Result<Vec<u8>, String>>,
    export_error: Option<String>,
}

impl Default for HyprBoardApp {
    fn default() -> Self {
        let (pdf_tx, pdf_rx) = mpsc::channel();
        Self {
            board: Board::default(),
            context_menu_pos: None,
            current_file: None,
            last_change: None,
            pending_action: PendingAction::None,
            show_unsaved_dialog: false,
            recent_files: RecentFiles::load(),
            show_toolbar: true,
            last_title: String::new(),
            show_pdf_dialog: false,
            pdf_mode: PdfMode::default(),
            pdf_page_size: PageSize::default(),
            pending_pdf: None,
            pdf_tx,
            pdf_rx,
            export_error: None,
        }
    }
}

impl eframe::App for HyprBoardApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        self.handle_global_shortcuts(ctx);
        self.board.handle_input(ctx);
        self.board.poll_downloads(ctx);

        // Track changes for autosave
        if self.board.is_dirty() && self.last_change.is_none() {
            self.last_change = Some(Instant::now());
        }

        self.update_title(ctx);
        self.show_menu_bar(ctx);

        if self.show_toolbar {
            self.show_toolbar(ctx);
        }

        let pointer_outside_canvas = ctx.input(|i| i.pointer.latest_pos()).is_some_and(|pos| {
            // Suppress if pointer is over a floating window/popup
            ctx.layer_id_at(pos).is_some_and(|id| id.order != egui::Order::Background)
                // Or if pointer is above the canvas area (over menu/toolbar)
                || pos.y < self.board.widget_rect_top()
        });
        self.board.suppress_input = self.context_menu_pos.is_some() || pointer_outside_canvas;

        CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_gray(30)))
            .show(ctx, |ui| {
                self.board.show(ui);

                let any_click = ui.input(|i| i.pointer.secondary_clicked());
                if any_click && let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    self.board.select_at(pos);
                    self.context_menu_pos = Some(pos);
                }
            });

        self.show_context_menu(ctx);
        self.show_unsaved_dialog(ctx);
        self.handle_dropped_files(ctx);
        self.show_drop_highlight(ctx);
        self.handle_pending_export();
        self.poll_pdf_export();
        self.handle_pending_pdf();
        self.show_pdf_export_dialog(ctx);
        self.show_export_error(ctx);
        self.handle_autosave();

        // Intercept close
        if ctx.input(|i| i.viewport().close_requested()) && self.board.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_action = PendingAction::Quit;
            self.show_unsaved_dialog = true;
        }
    }
}

impl HyprBoardApp {
    fn update_title(&mut self, ctx: &Context) {
        let title = match &self.current_file {
            Some(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "untitled".into());
                let dot = if self.board.is_dirty() {
                    " \u{2022}"
                } else {
                    ""
                };
                format!("HyprBoard \u{2014} {name}{dot}")
            }
            None => {
                if self.board.is_dirty() {
                    "HyprBoard \u{2014} untitled \u{2022}".into()
                } else {
                    "HyprBoard".into()
                }
            }
        };
        if title != self.last_title {
            self.last_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }

    fn handle_global_shortcuts(&mut self, ctx: &Context) {
        let save = ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::S));
        let save_as = ctx
            .input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::S));
        let open = ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::O));
        let toggle_toolbar = ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::T));

        if save_as {
            self.save_as();
        } else if save {
            self.save();
        }

        if open {
            self.try_open(ctx);
        }

        if toggle_toolbar {
            self.show_toolbar = !self.show_toolbar;
        }
    }

    fn save(&mut self) {
        if let Some(ref path) = self.current_file {
            let path = path.clone();
            self.save_to(&path);
        } else {
            self.save_as();
        }
    }

    fn save_as(&mut self) {
        let path = rfd::FileDialog::new()
            .add_filter("HyprBoard", &["hboard"])
            .save_file();
        if let Some(mut path) = path {
            if path.extension().is_none() {
                path.set_extension("hboard");
            }
            self.save_to(&path);
            self.current_file = Some(path);
        }
    }

    fn save_to(&mut self, path: &std::path::Path) {
        match persistence::save_board(path, self.board.items()) {
            Ok(()) => {
                self.board.mark_clean();
                self.last_change = None;
                self.recent_files.add(path);
                let autosave = path.with_extension("hboard.autosave");
                let _ = std::fs::remove_file(autosave);
                log::info!("Saved to {}", path.display());
            }
            Err(e) => {
                log::error!("Save failed: {e}");
            }
        }
    }

    fn try_open(&mut self, ctx: &Context) {
        if self.board.is_dirty() {
            self.pending_action = PendingAction::Open;
            self.show_unsaved_dialog = true;
        } else {
            self.do_open(ctx);
        }
    }

    fn do_open(&mut self, ctx: &Context) {
        let files = rfd::FileDialog::new()
            .add_filter(
                "All Supported",
                &["hboard", "bee", "png", "jpg", "jpeg", "bmp", "gif", "webp"],
            )
            .add_filter("HyprBoard Files", &["hboard"])
            .add_filter("BeeRef Files", &["bee"])
            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "webp"])
            .pick_file();

        if let Some(path) = files {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            match ext {
                "hboard" => self.load_board(ctx, &path),
                "bee" => self.import_bee(&path),
                _ => self.board.open_file_dialog_with_paths(ctx, &[path]),
            }
        }
    }

    fn load_board(&mut self, _ctx: &Context, path: &std::path::Path) {
        match persistence::load_board(path) {
            Ok(items) => {
                self.board.replace_items(items);
                self.current_file = Some(path.to_path_buf());
                self.last_change = None;
                self.recent_files.add(path);
                log::info!("Loaded {}", path.display());
            }
            Err(e) => {
                log::error!("Load failed: {e}");
            }
        }
    }

    fn import_bee(&mut self, path: &std::path::Path) {
        match crate::bee_import::import_bee(path) {
            Ok(items) => {
                self.board.replace_items(items);
                self.current_file = None;
                self.last_change = None;
                log::info!("Imported BeeRef file {}", path.display());
            }
            Err(e) => {
                log::error!("BeeRef import failed: {e}");
            }
        }
    }

    fn handle_autosave(&mut self) {
        if !self.board.is_dirty() {
            return;
        }
        let Some(ref file) = self.current_file else {
            return;
        };
        let Some(last) = self.last_change else {
            return;
        };
        if last.elapsed().as_secs() < 2 {
            return;
        }

        let autosave_path = file.with_extension("hboard.autosave");
        match persistence::save_board(&autosave_path, self.board.items()) {
            Ok(()) => {
                self.last_change = Some(Instant::now());
                log::debug!("Autosaved to {}", autosave_path.display());
            }
            Err(e) => {
                log::error!("Autosave failed: {e}");
            }
        }
    }

    fn show_unsaved_dialog(&mut self, ctx: &Context) {
        if !self.show_unsaved_dialog {
            return;
        }

        egui::Window::new("Unsaved Changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("You have unsaved changes. What would you like to do?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.save();
                        self.show_unsaved_dialog = false;
                        if !self.board.is_dirty() {
                            self.execute_pending(ctx);
                        } else {
                            self.pending_action = PendingAction::None;
                        }
                    }
                    if ui.button("Discard").clicked() {
                        self.board.mark_clean();
                        self.show_unsaved_dialog = false;
                        self.execute_pending(ctx);
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_unsaved_dialog = false;
                        self.pending_action = PendingAction::None;
                    }
                });
            });
    }

    fn execute_pending(&mut self, ctx: &Context) {
        match std::mem::take(&mut self.pending_action) {
            PendingAction::None => {}
            PendingAction::Open => self.do_open(ctx),
            PendingAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn icon(icon: &str) -> egui::RichText {
        egui::RichText::new(icon).size(18.0)
    }

    fn toolbar_btn(ui: &mut egui::Ui, icon: egui::RichText, label: &str) -> egui::Response {
        ui.button(icon).on_hover_text(label)
    }

    fn show_toolbar(&mut self, ctx: &Context) {
        use egui_phosphor::regular::*;

        let has_sel = self.board.has_selection();
        let has_img = self.board.has_image_selected();
        let has_txt = self.board.has_text_selected();

        TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                // File ops
                if Self::toolbar_btn(ui, Self::icon(FOLDER_OPEN), "Open (Ctrl+O)").clicked() {
                    self.try_open(ctx);
                }
                if Self::toolbar_btn(ui, Self::icon(FLOPPY_DISK), "Save (Ctrl+S)").clicked() {
                    self.save();
                }

                ui.separator();

                if Self::toolbar_btn(ui, Self::icon(ARROW_COUNTER_CLOCKWISE), "Undo (Ctrl+Z)")
                    .clicked()
                {
                    self.board.undo();
                }
                if Self::toolbar_btn(ui, Self::icon(ARROW_CLOCKWISE), "Redo (Ctrl+Shift+Z)")
                    .clicked()
                {
                    self.board.redo();
                }

                ui.separator();

                if Self::toolbar_btn(ui, Self::icon(ARROWS_OUT), "Zoom to Fit (F)").clicked() {
                    self.board.fit_all();
                }

                // Image controls (context-sensitive)
                if has_img {
                    ui.separator();

                    if Self::toolbar_btn(ui, Self::icon(CROP), "Crop (C)").clicked() {
                        self.board.start_crop();
                    }
                    if Self::toolbar_btn(ui, Self::icon(ARROWS_HORIZONTAL), "Flip H (Alt+H)")
                        .clicked()
                    {
                        self.board.flip_selected(true);
                    }
                    if Self::toolbar_btn(ui, Self::icon(ARROWS_VERTICAL), "Flip V (Alt+V)")
                        .clicked()
                    {
                        self.board.flip_selected(false);
                    }
                    if Self::toolbar_btn(ui, Self::icon(DROP_HALF_BOTTOM), "Grayscale (Alt+G)")
                        .clicked()
                    {
                        self.board.grayscale_selected();
                    }

                    let mut opacity = self.board.selected_opacity().unwrap_or(1.0);
                    let old_opacity = opacity;
                    ui.add(egui::Slider::new(&mut opacity, 0.0..=1.0).text("Opacity"));
                    if (opacity - old_opacity).abs() > 0.001 {
                        self.board.apply_opacity_selected(opacity);
                    }
                }

                // Z-order (context-sensitive)
                if has_sel {
                    ui.separator();

                    if Self::toolbar_btn(ui, Self::icon(ARROW_FAT_LINES_UP), "Bring to Front")
                        .clicked()
                    {
                        self.board.bring_to_front();
                    }
                    if Self::toolbar_btn(ui, Self::icon(ARROW_FAT_UP), "Raise (])").clicked() {
                        self.board.raise_selected();
                    }
                    if Self::toolbar_btn(ui, Self::icon(ARROW_FAT_DOWN), "Lower ([)").clicked() {
                        self.board.lower_selected();
                    }
                    if Self::toolbar_btn(ui, Self::icon(ARROW_FAT_LINES_DOWN), "Send to Back")
                        .clicked()
                    {
                        self.board.send_to_back();
                    }
                }

                // Text controls (context-sensitive)
                if has_txt {
                    ui.separator();

                    let mut font_size = self.board.selected_text_font_size().unwrap_or(16.0);
                    let old_font_size = font_size;
                    ui.label("Size");
                    ui.add(
                        egui::DragValue::new(&mut font_size)
                            .range(8.0..=128.0)
                            .speed(0.5),
                    );

                    let mut color = self.board.selected_text_color().unwrap_or(Color32::WHITE);
                    let old_color = color;
                    ui.label("Color");
                    egui::color_picker::color_edit_button_srgba(
                        ui,
                        &mut color,
                        egui::color_picker::Alpha::Opaque,
                    );

                    let mut bg_color = self
                        .board
                        .selected_text_bg_color()
                        .unwrap_or(Color32::TRANSPARENT);
                    let old_bg = bg_color;
                    ui.label("Bg");
                    egui::color_picker::color_edit_button_srgba(
                        ui,
                        &mut bg_color,
                        egui::color_picker::Alpha::BlendOrAdditive,
                    );

                    let changed = (font_size - old_font_size).abs() > 0.01
                        || color != old_color
                        || bg_color != old_bg;
                    if changed {
                        self.board.apply_text_style(font_size, color, bg_color);
                    }
                }

                // Border (universal, context-sensitive)
                if has_sel {
                    if !has_txt {
                        ui.separator();
                    }

                    let mut border = self
                        .board
                        .selected_border_color()
                        .unwrap_or(Color32::TRANSPARENT);
                    let old_border = border;
                    ui.label("Border");
                    egui::color_picker::color_edit_button_srgba(
                        ui,
                        &mut border,
                        egui::color_picker::Alpha::BlendOrAdditive,
                    );

                    if border != old_border {
                        self.board.apply_border_color(border);
                    }
                }

                ui.separator();

                let snap_label = if self.board.snap_to_grid {
                    "Snap to Grid: ON (Ctrl+Shift+G)"
                } else {
                    "Snap to Grid: OFF (Ctrl+Shift+G)"
                };
                let snap_icon = if self.board.snap_to_grid {
                    Self::icon(MAGNET_STRAIGHT).color(SELECTION_COLOR)
                } else {
                    Self::icon(MAGNET_STRAIGHT)
                };
                if Self::toolbar_btn(ui, snap_icon, snap_label).clicked() {
                    self.board.snap_to_grid = !self.board.snap_to_grid;
                }

                ui.separator();

                if Self::toolbar_btn(ui, Self::icon(EXPORT), "Export Region (Ctrl+E)").clicked() {
                    self.board.start_export_region();
                }
                if Self::toolbar_btn(ui, Self::icon(FILE_PDF), "Export PDF").clicked() {
                    self.show_pdf_dialog = true;
                }
                if Self::toolbar_btn(ui, Self::icon(SELECTION), "Screen Capture (Shift+S)")
                    .clicked()
                {
                    self.board.start_screen_capture(ctx);
                }
            });
            ui.add_space(4.0);
        });

        // Commit on pointer release
        if has_sel && ctx.input(|i| i.pointer.any_released()) {
            self.board.commit_text_style();
            self.board.commit_border_color();
            self.board.commit_opacity_selected();
        }
    }

    fn show_menu_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add(egui::Button::new("Open...").shortcut_text("Ctrl+O"))
                        .clicked()
                    {
                        ui.close();
                        self.try_open(ctx);
                    }

                    if ui.button("Import .bee...").clicked() {
                        ui.close();
                        let file = rfd::FileDialog::new()
                            .add_filter("BeeRef Files", &["bee"])
                            .pick_file();
                        if let Some(path) = file {
                            self.import_bee(&path);
                        }
                    }

                    ui.separator();

                    if ui
                        .add(egui::Button::new("Save").shortcut_text("Ctrl+S"))
                        .clicked()
                    {
                        ui.close();
                        self.save();
                    }

                    if ui
                        .add(egui::Button::new("Save As...").shortcut_text("Ctrl+Shift+S"))
                        .clicked()
                    {
                        ui.close();
                        self.save_as();
                    }

                    ui.separator();

                    if ui
                        .add(egui::Button::new("Export Region...").shortcut_text("Ctrl+E"))
                        .clicked()
                    {
                        ui.close();
                        self.board.start_export_region();
                    }

                    if ui.button("Export PDF...").clicked() {
                        ui.close();
                        self.show_pdf_dialog = true;
                    }

                    if ui
                        .add(egui::Button::new("Screen Capture").shortcut_text("Shift+S"))
                        .clicked()
                    {
                        ui.close();
                        self.board.start_screen_capture(ctx);
                    }

                    ui.separator();

                    let recent: Vec<PathBuf> = self.recent_files.entries().to_vec();
                    if !recent.is_empty() {
                        ui.menu_button("Recent Files", |ui| {
                            for path in &recent {
                                let label = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| path.display().to_string());
                                if ui
                                    .button(&label)
                                    .on_hover_text(path.display().to_string())
                                    .clicked()
                                {
                                    ui.close();
                                    let path = path.clone();
                                    let ext =
                                        path.extension().and_then(|e| e.to_str()).unwrap_or("");
                                    match ext {
                                        "hboard" => self.load_board(ctx, &path),
                                        "bee" => self.import_bee(&path),
                                        _ => {}
                                    }
                                }
                            }
                        });

                        ui.separator();
                    }

                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui
                        .add(egui::Button::new("Undo").shortcut_text("Ctrl+Z"))
                        .clicked()
                    {
                        ui.close();
                        self.board.undo();
                    }

                    if ui
                        .add(egui::Button::new("Redo").shortcut_text("Ctrl+Shift+Z"))
                        .clicked()
                    {
                        ui.close();
                        self.board.redo();
                    }

                    ui.separator();

                    let has_sel = self.board.has_selection();

                    if ui.add_enabled(has_sel, egui::Button::new("Copy")).clicked() {
                        ui.close();
                        self.board.copy_selected();
                    }

                    if ui.add_enabled(has_sel, egui::Button::new("Cut")).clicked() {
                        ui.close();
                        self.board.cut_selection();
                    }

                    if ui
                        .add(egui::Button::new("Paste").shortcut_text("Ctrl+V"))
                        .clicked()
                    {
                        ui.close();
                        self.board.paste_from_clipboard(ctx);
                    }

                    if ui
                        .add_enabled(
                            has_sel,
                            egui::Button::new("Delete Selected").shortcut_text("Del"),
                        )
                        .clicked()
                    {
                        ui.close();
                        self.board.delete_selected();
                    }
                });

                ui.menu_button("View", |ui| {
                    let toolbar_label = if self.show_toolbar {
                        "Hide Toolbar"
                    } else {
                        "Show Toolbar"
                    };
                    if ui
                        .add(egui::Button::new(toolbar_label).shortcut_text("Ctrl+T"))
                        .clicked()
                    {
                        ui.close();
                        self.show_toolbar = !self.show_toolbar;
                    }

                    ui.separator();

                    let grid_label = if self.board.show_grid {
                        "Hide Grid"
                    } else {
                        "Show Grid"
                    };
                    if ui
                        .add(egui::Button::new(grid_label).shortcut_text("Ctrl+G"))
                        .clicked()
                    {
                        ui.close();
                        self.board.show_grid = !self.board.show_grid;
                    }

                    let snap_label = if self.board.snap_to_grid {
                        "Disable Snap"
                    } else {
                        "Enable Snap"
                    };
                    if ui
                        .add(egui::Button::new(snap_label).shortcut_text("Ctrl+Shift+G"))
                        .clicked()
                    {
                        ui.close();
                        self.board.snap_to_grid = !self.board.snap_to_grid;
                    }

                    ui.separator();

                    if ui.button("Zoom to Fit").clicked() {
                        ui.close();
                        self.board.fit_all();
                    }
                });

                if let Some(ref path) = self.current_file {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    let dirty = if self.board.is_dirty() {
                        " \u{2022}"
                    } else {
                        ""
                    };
                    ui.label(
                        egui::RichText::new(format!("{name}{dirty}"))
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let count = self.board.item_count();
                    let sel = self.board.selection_count();
                    let status = if sel > 0 {
                        format!("{count} items | {sel} selected")
                    } else {
                        format!("{count} items")
                    };
                    ui.label(
                        egui::RichText::new(status)
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                });
            });
        });
    }

    fn show_context_menu(&mut self, ctx: &Context) {
        let Some(pos) = self.context_menu_pos else {
            return;
        };

        let menu_id = egui::Id::new("canvas_context_menu");
        let mut close = false;

        let area_resp = egui::Area::new(menu_id)
            .fixed_pos(pos)
            .constrain(true)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(140.0);

                    if self.board.has_selection() {
                        // Selection context menu (flat)
                        if ui.button("Copy").clicked() {
                            self.board.copy_selected();
                            close = true;
                        }
                        if ui.button("Cut").clicked() {
                            self.board.cut_selection();
                            close = true;
                        }
                        if ui.button("Paste").clicked() {
                            self.board.paste_from_clipboard(ctx);
                            close = true;
                        }
                        if ui.button("Delete").clicked() {
                            self.board.delete_selected();
                            close = true;
                        }

                        ui.separator();

                        if ui.button("Add Label").clicked() {
                            self.board.add_label_to_selected();
                            close = true;
                        }
                    } else {
                        // Background context menu — mirrors File menu
                        if ui
                            .add(egui::Button::new("Open...").shortcut_text("Ctrl+O"))
                            .clicked()
                        {
                            close = true;
                            self.try_open(ctx);
                        }

                        if ui.button("Import .bee...").clicked() {
                            close = true;
                            let file = rfd::FileDialog::new()
                                .add_filter("BeeRef Files", &["bee"])
                                .pick_file();
                            if let Some(path) = file {
                                self.import_bee(&path);
                            }
                        }

                        ui.separator();

                        if ui
                            .add(egui::Button::new("Save").shortcut_text("Ctrl+S"))
                            .clicked()
                        {
                            close = true;
                            self.save();
                        }

                        if ui
                            .add(egui::Button::new("Save As...").shortcut_text("Ctrl+Shift+S"))
                            .clicked()
                        {
                            close = true;
                            self.save_as();
                        }

                        ui.separator();

                        if ui
                            .add(egui::Button::new("Export Region...").shortcut_text("Ctrl+E"))
                            .clicked()
                        {
                            close = true;
                            self.board.start_export_region();
                        }

                        if ui.button("Export PDF...").clicked() {
                            close = true;
                            self.show_pdf_dialog = true;
                        }

                        if ui
                            .add(egui::Button::new("Screen Capture").shortcut_text("Shift+S"))
                            .clicked()
                        {
                            close = true;
                            self.board.start_screen_capture(ctx);
                        }

                        if ui
                            .add(egui::Button::new("Paste").shortcut_text("Ctrl+V"))
                            .clicked()
                        {
                            close = true;
                            self.board.paste_from_clipboard(ctx);
                        }

                        if ui.button("Add Text").clicked() {
                            close = true;
                            self.board.add_text_at(pos);
                        }

                        ui.separator();

                        let recent: Vec<PathBuf> = self.recent_files.entries().to_vec();
                        if !recent.is_empty() {
                            ui.menu_button("Recent Files", |ui| {
                                for path in &recent {
                                    let label = path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_else(|| path.display().to_string());
                                    if ui
                                        .button(&label)
                                        .on_hover_text(path.display().to_string())
                                        .clicked()
                                    {
                                        ui.close();
                                        close = true;
                                        let path = path.clone();
                                        let ext =
                                            path.extension().and_then(|e| e.to_str()).unwrap_or("");
                                        match ext {
                                            "hboard" => self.load_board(ctx, &path),
                                            "bee" => self.import_bee(&path),
                                            _ => {}
                                        }
                                    }
                                }
                            });

                            ui.separator();
                        }

                        if ui.button("Quit").clicked() {
                            close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                });
            });

        if close {
            self.context_menu_pos = None;
            return;
        }

        let primary_elsewhere = ctx
            .input(|i| i.pointer.button_clicked(egui::PointerButton::Primary))
            && !area_resp.response.contains_pointer();
        if primary_elsewhere || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.context_menu_pos = None;
        }
    }

    fn handle_dropped_files(&mut self, ctx: &Context) {
        let dropped: Vec<egui::DroppedFile> = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }

        let drop_pos = ctx
            .input(|i| i.pointer.latest_pos())
            .map(|p| self.board.screen_to_scene(p))
            .unwrap_or_else(|| {
                let vr = self.board.visible_rect();
                Vec2::new(vr.center().x, vr.center().y)
            });

        for file in dropped {
            let path_str = file
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            // Handle HTTP/HTTPS URLs (e.g. image dragged from Chrome)
            if path_str.starts_with("http://") || path_str.starts_with("https://") {
                log::info!("Downloading dropped URL: {path_str}");
                self.board.spawn_download(ctx, &path_str, drop_pos);
                continue;
            }

            let ext = file
                .path
                .as_ref()
                .and_then(|p| p.extension())
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            match ext.as_str() {
                "hboard" => {
                    if let Some(path) = &file.path {
                        self.load_board(ctx, path);
                    }
                }
                "bee" => {
                    if let Some(path) = &file.path {
                        self.import_bee(path);
                    }
                }
                _ => {
                    if let Some(bytes) = file.bytes {
                        self.board.add_image_from_bytes(ctx, &bytes, drop_pos);
                    } else if let Some(path) = file.path
                        && let Ok(bytes) = std::fs::read(&path)
                    {
                        self.board.add_image_from_bytes(ctx, &bytes, drop_pos);
                    }
                }
            }
        }
    }

    fn handle_pending_export(&mut self) {
        if let Some(png) = self.board.take_pending_export() {
            let path = rfd::FileDialog::new()
                .add_filter("PNG", &["png"])
                .set_file_name("export.png")
                .save_file();
            if let Some(path) = path {
                if let Err(e) = std::fs::write(&path, &png) {
                    log::error!("Export save failed: {e}");
                } else {
                    log::info!("Exported to {}", path.display());
                }
            }
        }
    }

    fn show_pdf_export_dialog(&mut self, ctx: &Context) {
        if !self.show_pdf_dialog {
            return;
        }

        egui::Window::new("Export PDF")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Mode:");
                    for mode in PdfMode::ALL {
                        ui.selectable_value(&mut self.pdf_mode, mode, mode.label());
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Page Size:");
                    for size in PageSize::ALL {
                        ui.selectable_value(&mut self.pdf_page_size, size, size.label());
                    }
                });

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if ui.button("Export").clicked() {
                        self.show_pdf_dialog = false;
                        self.do_pdf_export();
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_pdf_dialog = false;
                    }
                });
            });
    }

    fn do_pdf_export(&mut self) {
        let items = self.board.items().to_vec();
        if items.is_empty() {
            self.export_error = Some("No items to export".into());
            return;
        }

        let mode = self.pdf_mode;
        let page_size = self.pdf_page_size;
        let tx = self.pdf_tx.clone();
        std::thread::spawn(move || {
            let result = crate::pdf_export::export_pdf(&items, mode, page_size);
            let _ = tx.send(result);
        });
    }

    fn poll_pdf_export(&mut self) {
        if let Ok(result) = self.pdf_rx.try_recv() {
            match result {
                Ok(bytes) => self.pending_pdf = Some(bytes),
                Err(e) => {
                    log::error!("PDF export failed: {e}");
                    self.export_error = Some(format!("PDF export failed: {e}"));
                }
            }
        }
    }

    fn handle_pending_pdf(&mut self) {
        if let Some(pdf) = self.pending_pdf.take() {
            let path = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_file_name("export.pdf")
                .save_file();
            if let Some(path) = path {
                if let Err(e) = std::fs::write(&path, &pdf) {
                    log::error!("PDF write failed: {e}");
                } else {
                    log::info!("PDF exported to {}", path.display());
                }
            }
        }
    }

    fn show_export_error(&mut self, ctx: &Context) {
        let Some(ref msg) = self.export_error else {
            return;
        };
        let msg = msg.clone();

        let mut open = true;
        egui::Window::new("Export Error")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(&msg);
                ui.add_space(8.0);
                if ui.button("OK").clicked() {
                    self.export_error = None;
                }
            });

        if !open {
            self.export_error = None;
        }
    }

    fn show_drop_highlight(&self, ctx: &Context) {
        let has_hovered = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !has_hovered {
            return;
        }

        let screen_rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop_highlight"),
        ));

        painter.rect(
            screen_rect.shrink(4.0),
            8.0,
            egui::Color32::from_rgba_premultiplied(100, 160, 255, 30),
            egui::Stroke::new(3.0, egui::Color32::from_rgb(100, 160, 255)),
            egui::StrokeKind::Inside,
        );

        painter.text(
            screen_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop files here",
            egui::FontId::proportional(24.0),
            egui::Color32::from_rgb(100, 160, 255),
        );
    }
}
