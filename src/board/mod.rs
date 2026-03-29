mod interaction;
pub(crate) mod render;
mod undo;

use std::collections::HashSet;
use std::sync::mpsc;

use egui::{
    Key, Modifiers, Pos2, Rect, Ui, Vec2,
    containers::{DragPanButtons, Scene},
};

use crate::items::{BoardItem, Label, load_image_from_bytes};

use interaction::InteractionState;
use undo::{Command, UndoStack};

use crate::clipboard;

pub struct Board {
    items: Vec<BoardItem>,
    selected: HashSet<usize>,
    scene_rect: Rect,
    interaction: InteractionState,
    undo_stack: UndoStack,
    next_texture_id: u64,
    pub show_grid: bool,
    pub snap_to_grid: bool,
    pub grid_size: f32,
    visible_rect: Rect,
    widget_rect: Rect,
    download_tx: mpsc::Sender<(Vec<u8>, Vec2)>,
    download_rx: mpsc::Receiver<(Vec<u8>, Vec2)>,
    pending_export: Option<Vec<u8>>,
    pending_zoom: Option<Rect>,
    pub suppress_input: bool,
}

impl Default for Board {
    fn default() -> Self {
        let (download_tx, download_rx) = mpsc::channel();
        Self {
            items: Vec::new(),
            selected: HashSet::new(),
            scene_rect: Rect::NOTHING,
            interaction: InteractionState::Idle,
            undo_stack: UndoStack::default(),
            next_texture_id: 0,
            show_grid: true,
            snap_to_grid: true,
            grid_size: 50.0,
            visible_rect: Rect::NOTHING,
            widget_rect: Rect::NOTHING,
            download_tx,
            download_rx,
            pending_export: None,
            pending_zoom: None,
            suppress_input: false,
        }
    }
}

impl Board {
    fn next_name(&mut self) -> String {
        self.next_texture_id += 1;
        format!("img-{}", self.next_texture_id)
    }

    fn cascade_pos(&self) -> Vec2 {
        Vec2::splat(self.items.len() as f32 * 20.0)
    }

    pub fn visible_rect(&self) -> Rect {
        self.visible_rect
    }

    /// Convert a screen-space position to scene-space coordinates.
    pub fn screen_to_scene(&self, screen_pos: Pos2) -> Vec2 {
        let scale_x = self.widget_rect.width() / self.scene_rect.width();
        let scale_y = self.widget_rect.height() / self.scene_rect.height();
        let scale = scale_x.min(scale_y).clamp(0.05, 5.0);

        let center_screen = self.widget_rect.center().to_vec2();
        let center_scene = self.scene_rect.center().to_vec2();

        (screen_pos.to_vec2() - center_screen) / scale + center_scene
    }

    pub fn add_image_from_bytes(&mut self, ctx: &egui::Context, bytes: &[u8], position: Vec2) {
        let name = self.next_name();
        if let Some((texture, size, original_bytes)) = load_image_from_bytes(ctx, &name, bytes) {
            self.items.push(BoardItem::new_image(
                texture,
                original_bytes,
                size,
                position,
            ));
            self.undo_stack.push(Command::Add { count: 1 });
        }
    }

    pub fn delete_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let mut indices: Vec<usize> = self.selected.iter().copied().collect();
        indices.sort_unstable();
        let mut removed = Vec::new();
        for &idx in indices.iter().rev() {
            if idx < self.items.len() {
                removed.push((idx, self.items.remove(idx)));
            }
        }
        removed.reverse();
        self.undo_stack.push(Command::Delete { items: removed });
        self.selected.clear();
    }

    pub fn copy_selected(&self) {
        if self.selected.is_empty() {
            return;
        }

        if self.selected.len() == 1 {
            let idx = *self.selected.iter().next().unwrap();
            if let Some(item) = self.items.get(idx) {
                match item {
                    BoardItem::Image(img) => match clipboard::ensure_png(&img.original_bytes) {
                        Ok(png) => {
                            if let Err(e) = clipboard::copy_image_to_clipboard(&png) {
                                log::error!("Copy failed: {e}");
                            }
                        }
                        Err(e) => log::error!("PNG encode: {e}"),
                    },
                    BoardItem::Text(txt) => {
                        if let Err(e) = clipboard::copy_text_to_clipboard(&txt.content) {
                            log::error!("Copy text failed: {e}");
                        }
                    }
                }
            }
            return;
        }

        // Multi-select: render collage
        let mut selected_items: Vec<(usize, &BoardItem)> = self
            .selected
            .iter()
            .filter_map(|&i| self.items.get(i).map(|item| (i, item)))
            .collect();
        selected_items.sort_by_key(|(i, _)| *i);
        let refs: Vec<&BoardItem> = selected_items.into_iter().map(|(_, item)| item).collect();

        match clipboard::render_collage(&refs) {
            Ok(png) => {
                if let Err(e) = clipboard::copy_image_to_clipboard(&png) {
                    log::error!("Copy collage failed: {e}");
                }
            }
            Err(e) => log::error!("Collage render: {e}"),
        }
    }

    pub fn cut_selection(&mut self) {
        self.copy_selected();
        self.delete_selected();
    }

    pub fn open_file_dialog_with_paths(
        &mut self,
        ctx: &egui::Context,
        paths: &[std::path::PathBuf],
    ) {
        let mut added = 0;
        for path in paths {
            if let Ok(bytes) = std::fs::read(path) {
                let name = format!(
                    "file-{}",
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".into())
                );
                if let Some((texture, size, original_bytes)) =
                    load_image_from_bytes(ctx, &name, &bytes)
                {
                    let pos = self.cascade_pos();
                    self.items
                        .push(BoardItem::new_image(texture, original_bytes, size, pos));
                    added += 1;
                }
            }
        }
        if added > 0 {
            self.undo_stack.push(Command::Add { count: added });
        }
    }

    pub fn paste_from_clipboard(&mut self, ctx: &egui::Context) {
        if let Some(bytes) = Self::wl_paste_image() {
            let pos = self.cascade_pos();
            self.add_image_from_bytes(ctx, &bytes, pos);
            return;
        }

        if let Some(text) = Self::wl_paste_text() {
            let text = text.trim();

            // Handle file:// URIs (e.g. from file manager copy)
            if text.starts_with("file://") {
                let mut loaded = 0;
                for line in text.lines() {
                    let line = line.trim();
                    if let Some(path_str) = line.strip_prefix("file://") {
                        let decoded = crate::util::urlencoding_decode(path_str);
                        let path = std::path::PathBuf::from(&decoded);
                        if path.exists()
                            && let Ok(bytes) = std::fs::read(&path)
                        {
                            let pos = self.cascade_pos();
                            self.add_image_from_bytes(ctx, &bytes, pos);
                            loaded += 1;
                        }
                    }
                }
                if loaded > 0 {
                    return;
                }
            }

            if text.starts_with("http://") || text.starts_with("https://") {
                let pos = self.cascade_pos();
                self.spawn_download(ctx, text, pos);
            }
        }
    }

    fn wl_paste_image() -> Option<Vec<u8>> {
        use std::process::Command;
        let output = Command::new("wl-paste")
            .args(["--type", "image/png", "--no-newline"])
            .output()
            .ok()?;
        if output.status.success() && !output.stdout.is_empty() {
            Some(output.stdout)
        } else {
            None
        }
    }

    fn wl_paste_text() -> Option<String> {
        use std::process::Command;
        let output = Command::new("wl-paste")
            .args(["--type", "text/plain", "--no-newline"])
            .output()
            .ok()?;
        if output.status.success() && !output.stdout.is_empty() {
            String::from_utf8(output.stdout).ok()
        } else {
            None
        }
    }

    pub fn spawn_download(&self, ctx: &egui::Context, url: &str, pos: Vec2) {
        let tx = self.download_tx.clone();
        let url = url.to_string();
        let ctx = ctx.clone();
        std::thread::spawn(move || match ureq::get(&url).call() {
            Ok(resp) => {
                if let Ok(bytes) = resp.into_body().read_to_vec() {
                    let _ = tx.send((bytes, pos));
                    ctx.request_repaint();
                }
            }
            Err(e) => log::error!("Download failed: {e}"),
        });
    }

    pub fn start_screen_capture(&self, ctx: &egui::Context) {
        let tx = self.download_tx.clone();
        let ctx = ctx.clone();
        let pos = self.scene_rect.center().to_vec2();
        std::thread::spawn(move || {
            let region = match std::process::Command::new("slurp").output() {
                Ok(out) if out.status.success() => {
                    String::from_utf8_lossy(&out.stdout).trim().to_string()
                }
                _ => return,
            };
            match std::process::Command::new("grim")
                .args(["-g", &region, "-t", "png", "-"])
                .output()
            {
                Ok(out) if out.status.success() => {
                    let _ = tx.send((out.stdout, pos));
                    ctx.request_repaint();
                }
                Ok(out) => {
                    log::error!("grim failed: {}", String::from_utf8_lossy(&out.stderr));
                }
                Err(e) => log::error!("Failed to run grim: {e}"),
            }
        });
    }

    pub fn poll_downloads(&mut self, ctx: &egui::Context) {
        while let Ok((bytes, pos)) = self.download_rx.try_recv() {
            self.add_image_from_bytes(ctx, &bytes, pos);
        }
    }

    pub fn add_text_at(&mut self, screen_pos: Pos2) {
        let pos = self.screen_to_scene(screen_pos);
        self.items.push(BoardItem::new_text("Text".into(), pos));
        let idx = self.items.len() - 1;
        self.undo_stack.push(Command::Add { count: 1 });
        self.selected.clear();
        self.selected.insert(idx);
        self.interaction = InteractionState::EditingText { idx };
    }

    pub fn take_pending_export(&mut self) -> Option<Vec<u8>> {
        self.pending_export.take()
    }

    pub fn undo(&mut self) {
        self.undo_stack.undo(&mut self.items, &mut self.selected);
    }

    pub fn redo(&mut self) {
        self.undo_stack.redo(&mut self.items, &mut self.selected);
    }

    fn raise_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let old_order: Vec<usize> = (0..self.items.len()).collect();
        let mut indices: Vec<usize> = self.selected.iter().copied().collect();
        indices.sort_unstable();

        for &idx in indices.iter().rev() {
            if idx + 1 < self.items.len() && !self.selected.contains(&(idx + 1)) {
                self.items.swap(idx, idx + 1);
                self.selected.remove(&idx);
                self.selected.insert(idx + 1);
            }
        }
        self.undo_stack.push(Command::ZOrder { old_order });
    }

    fn lower_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let old_order: Vec<usize> = (0..self.items.len()).collect();
        let mut indices: Vec<usize> = self.selected.iter().copied().collect();
        indices.sort_unstable();

        for &idx in &indices {
            if idx > 0 && !self.selected.contains(&(idx - 1)) {
                self.items.swap(idx, idx - 1);
                self.selected.remove(&idx);
                self.selected.insert(idx - 1);
            }
        }
        self.undo_stack.push(Command::ZOrder { old_order });
    }

    pub fn set_opacity_selected(&mut self, new_opacity: f32) {
        if self.selected.is_empty() {
            return;
        }
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        let old_values: Vec<f32> = indices
            .iter()
            .filter_map(|&i| self.items.get(i).map(|item| item.opacity()))
            .collect();
        let new_values = vec![new_opacity; indices.len()];
        for &idx in &indices {
            if let Some(item) = self.items.get_mut(idx) {
                item.set_opacity(new_opacity);
            }
        }
        self.undo_stack.push(Command::Opacity {
            indices,
            old_values,
            new_values,
        });
    }

    pub fn selected_opacity(&self) -> Option<f32> {
        let mut iter = self.selected.iter().filter_map(|&i| self.items.get(i));
        let first = iter.next()?.opacity();
        if iter.all(|item| (item.opacity() - first).abs() < 0.01) {
            Some(first)
        } else {
            None
        }
    }

    pub fn grayscale_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for &idx in &indices {
            if let Some(item) = self.items.get_mut(idx) {
                item.toggle_grayscale();
            }
        }
        self.undo_stack.push(Command::Grayscale { indices });
    }

    pub fn flip_selected(&mut self, horizontal: bool) {
        if self.selected.is_empty() {
            return;
        }
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for &idx in &indices {
            if let Some(item) = self.items.get_mut(idx) {
                item.toggle_flip(horizontal);
            }
        }
        self.undo_stack.push(Command::Flip {
            indices,
            horizontal,
        });
    }

    pub fn add_label_to_selected(&mut self) {
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for idx in indices {
            if self
                .items
                .get(idx)
                .is_some_and(|i| matches!(i, BoardItem::Image(_)))
            {
                let label_count = self.items[idx].labels().len();
                let offset = Vec2::new(0.0, -20.0 * (label_count as f32 + 1.0));
                self.add_label_to_item(idx, offset);
            }
        }
    }

    pub fn add_label_to_item(&mut self, item_idx: usize, offset: Vec2) {
        if let Some(item) = self.items.get_mut(item_idx) {
            item.add_label(Label::new("Label".into(), offset));
            self.undo_stack.push(Command::AddLabel { item_idx });
        }
    }

    pub fn reset_crop_selected(&mut self) {
        for &idx in &self.selected {
            if let Some(item) = self.items.get_mut(idx) {
                let old = item.crop_rect();
                if old.is_some() {
                    item.set_crop_rect(None);
                    self.undo_stack.push(Command::Crop {
                        idx,
                        old_rect: old,
                        new_rect: None,
                    });
                }
            }
        }
    }

    pub fn fit_all(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if let Some(rect) = self.all_items_rect() {
            self.scene_rect = rect.expand(50.0);
        }
    }

    pub fn fit_selected(&mut self) {
        if let Some(rect) = interaction::selected_bounding_rect(&self.items, &self.selected) {
            self.scene_rect = rect.expand(50.0);
        }
    }

    fn all_items_rect(&self) -> Option<Rect> {
        let mut iter = self.items.iter().map(|item| item.bounding_rect());
        let first = iter.next()?;
        Some(iter.fold(first, |acc, r| acc.union(r)))
    }

    fn nudge_selected(&mut self, delta: Vec2) {
        if self.selected.is_empty() {
            return;
        }
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for &idx in &indices {
            if let Some(item) = self.items.get_mut(idx) {
                item.transform_mut().position += delta;
            }
        }
        self.undo_stack.push(Command::Move { indices, delta });
    }

    pub fn start_export_region(&mut self) {
        self.interaction = InteractionState::ExportingRegion {
            start: None,
            current: Pos2::ZERO,
        };
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let scene_rect = &mut self.scene_rect;
        let items = &mut self.items;
        let selected = &mut self.selected;
        let state = &mut self.interaction;
        let undo_stack = &mut self.undo_stack;
        let show_grid = self.show_grid;
        let grid_size = self.grid_size;
        let snap_to_grid = self.snap_to_grid;
        let visible_rect = &mut self.visible_rect;
        let widget_rect = &mut self.widget_rect;
        let next_texture_id = &mut self.next_texture_id;
        let pending_export = &mut self.pending_export;
        let pending_zoom = &mut self.pending_zoom;
        let suppress_input = self.suppress_input;

        *widget_rect = ui.max_rect();

        if *scene_rect == Rect::NOTHING {
            let size = widget_rect.size();
            *scene_rect = Rect::from_center_size(egui::Pos2::ZERO, size);
        }

        Scene::new()
            .zoom_range(0.05..=5.0)
            .drag_pan_buttons(DragPanButtons::MIDDLE)
            .show(ui, scene_rect, |scene_ui| {
                *visible_rect = scene_ui.clip_rect();

                interaction::render_scene(
                    scene_ui,
                    items,
                    selected,
                    state,
                    undo_stack,
                    show_grid,
                    grid_size,
                    snap_to_grid,
                    *visible_rect,
                    next_texture_id,
                    pending_export,
                    suppress_input,
                    pending_zoom,
                );
            });

        if let Some(rect) = self.pending_zoom.take() {
            self.scene_rect = rect;
        }
    }

    pub fn handle_input(&mut self, ctx: &egui::Context) {
        let do_delete = ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace));
        if do_delete {
            self.delete_selected();
        }

        let do_escape = ctx.input(|i| i.key_pressed(Key::Escape));
        if do_escape {
            self.selected.clear();
        }

        let raise = ctx.input(|i| i.key_pressed(Key::CloseBracket) && !i.modifiers.ctrl);
        let lower = ctx.input(|i| i.key_pressed(Key::OpenBracket) && !i.modifiers.ctrl);
        if raise {
            self.raise_selected();
        }
        if lower {
            self.lower_selected();
        }

        // Redo must be checked before undo (Ctrl+Shift+Z vs Ctrl+Z)
        let do_redo = ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Z));
        if do_redo {
            self.redo();
            return;
        }
        let do_undo = ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Z));
        if do_undo {
            self.undo();
        }

        // Remapped: Alt+H/V/G for flip/grayscale
        let flip_h = ctx.input(|i| i.key_pressed(Key::H) && i.modifiers.alt);
        let flip_v = ctx.input(|i| i.key_pressed(Key::V) && i.modifiers.alt);
        if flip_h {
            self.flip_selected(true);
        }
        if flip_v {
            self.flip_selected(false);
        }

        let toggle_gray = ctx.input(|i| i.key_pressed(Key::G) && i.modifiers.alt);
        if toggle_gray {
            self.grayscale_selected();
        }

        let fit_all =
            ctx.input(|i| i.key_pressed(Key::F) && !i.modifiers.ctrl && !i.modifiers.shift);
        let fit_sel =
            ctx.input(|i| i.key_pressed(Key::F) && i.modifiers.shift && !i.modifiers.ctrl);
        if fit_sel {
            self.fit_selected();
        } else if fit_all {
            self.fit_all();
        }

        let crop = ctx.input(|i| i.key_pressed(Key::C) && !i.modifiers.ctrl && !i.modifiers.shift);
        let reset_crop =
            ctx.input(|i| i.key_pressed(Key::C) && i.modifiers.shift && !i.modifiers.ctrl);
        if reset_crop {
            self.reset_crop_selected();
        } else if crop && self.selected.len() == 1 {
            let idx = *self.selected.iter().next().unwrap();
            if self
                .items
                .get(idx)
                .is_some_and(|item| matches!(item, BoardItem::Image(_)))
            {
                self.interaction = InteractionState::Cropping {
                    idx,
                    start: None,
                    current: Pos2::ZERO,
                };
            }
        }

        // Vim-style movement (only when items selected)
        if self.has_selection() {
            let nudge = if self.snap_to_grid {
                self.grid_size
            } else {
                10.0
            };

            let h = ctx.input(|i| i.key_pressed(Key::H) && !i.modifiers.alt && !i.modifiers.ctrl);
            let j = ctx.input(|i| i.key_pressed(Key::J) && !i.modifiers.ctrl);
            let k = ctx.input(|i| i.key_pressed(Key::K) && !i.modifiers.ctrl);
            let l = ctx.input(|i| i.key_pressed(Key::L) && !i.modifiers.alt && !i.modifiers.ctrl);

            if h {
                self.nudge_selected(Vec2::new(-nudge, 0.0));
            }
            if j {
                self.nudge_selected(Vec2::new(0.0, nudge));
            }
            if k {
                self.nudge_selected(Vec2::new(0.0, -nudge));
            }
            if l {
                self.nudge_selected(Vec2::new(nudge, 0.0));
            }

            let x_del = ctx.input(|i| i.key_pressed(Key::X) && !i.modifiers.ctrl);
            if x_del {
                self.delete_selected();
            }
        }

        // u = undo alias
        let u_undo = ctx.input(|i| i.key_pressed(Key::U) && !i.modifiers.ctrl);
        if u_undo {
            self.undo();
        }

        // Ctrl+G toggle grid, Ctrl+Shift+G toggle snap
        let toggle_grid = ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::G));
        let toggle_snap =
            ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::G));
        if toggle_snap {
            self.snap_to_grid = !self.snap_to_grid;
        } else if toggle_grid {
            self.show_grid = !self.show_grid;
        }

        // Ctrl+E export region
        let export = ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::E));
        if export {
            self.start_export_region();
        }

        // Shift+S screen capture
        let capture =
            ctx.input(|i| i.key_pressed(Key::S) && i.modifiers.shift && !i.modifiers.ctrl);
        if capture {
            self.start_screen_capture(ctx);
        }
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn selection_count(&self) -> usize {
        self.selected.len()
    }

    pub fn has_selection(&self) -> bool {
        !self.selected.is_empty()
    }

    /// Select the topmost item at `screen_pos`, replacing existing selection.
    /// Returns true if an item was found and selected.
    pub fn select_at(&mut self, screen_pos: egui::Pos2) -> bool {
        let scene_pos = self.screen_to_scene(screen_pos);
        for (idx, item) in self.items.iter().enumerate().rev() {
            if item.bounding_rect().contains(scene_pos.to_pos2()) {
                if !self.selected.contains(&idx) {
                    self.selected.clear();
                    self.selected.insert(idx);
                }
                return true;
            }
        }
        false
    }

    pub fn items(&self) -> &[BoardItem] {
        &self.items
    }

    pub fn replace_items(&mut self, new_items: Vec<BoardItem>) {
        self.items = new_items;
        self.selected.clear();
        self.undo_stack = UndoStack::default();
        self.interaction = InteractionState::Idle;
        let (tx, rx) = mpsc::channel();
        self.download_tx = tx;
        self.download_rx = rx;
        self.pending_export = None;
    }

    pub fn is_dirty(&self) -> bool {
        self.undo_stack.dirty
    }

    pub fn mark_clean(&mut self) {
        self.undo_stack.dirty = false;
    }
}
