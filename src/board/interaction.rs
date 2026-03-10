use std::collections::HashSet;

use egui::{Color32, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

use crate::clipboard;
use crate::items::BoardItem;

use super::render::{
    HANDLE_SIZE, HandleHit, ROT_HANDLE_OFFSET, draw_grid, draw_item, draw_selection_handles,
};
use super::undo::{Command, UndoStack};

const LAZY_DECODE_PER_FRAME: usize = 2;
const SELECTION_COLOR: Color32 = Color32::from_rgb(100, 160, 255);

// --- Interaction state ---

#[derive(Default)]
pub enum InteractionState {
    #[default]
    Idle,
    DraggingItems {
        drag_started: bool,
        last_pointer: Pos2,
        start_positions: Vec<(usize, Vec2)>,
    },
    SelectionRect {
        start: Pos2,
    },
    ResizingHandle {
        corner: Corner,
        start_mouse: Pos2,
        initial_scales: Vec<(usize, Vec2)>,
        initial_positions: Vec<(usize, Vec2)>,
        group_rect: Rect,
    },
    RotatingHandle {
        center: Pos2,
        start_angle: f32,
        initial_rotations: Vec<(usize, f32)>,
        initial_positions: Vec<(usize, Vec2)>,
    },
    Cropping {
        idx: usize,
        start: Option<Pos2>,
        current: Pos2,
    },
    EditingText {
        idx: usize,
    },
    DraggingLabel {
        item_idx: usize,
        label_idx: usize,
        start_offset: Vec2,
        last_pointer: Pos2,
    },
    EditingLabel {
        item_idx: usize,
        label_idx: usize,
    },
    ExportingRegion {
        start: Option<Pos2>,
        current: Pos2,
    },
}

#[derive(Clone, Copy, PartialEq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    pub fn pos_in_rect(self, rect: Rect) -> Pos2 {
        match self {
            Corner::TopLeft => rect.left_top(),
            Corner::TopRight => rect.right_top(),
            Corner::BottomLeft => rect.left_bottom(),
            Corner::BottomRight => rect.right_bottom(),
        }
    }

    pub fn opposite(self) -> Corner {
        match self {
            Corner::TopLeft => Corner::BottomRight,
            Corner::TopRight => Corner::BottomLeft,
            Corner::BottomLeft => Corner::TopRight,
            Corner::BottomRight => Corner::TopLeft,
        }
    }
}

pub fn selected_bounding_rect(items: &[BoardItem], selected: &HashSet<usize>) -> Option<Rect> {
    let mut rects = selected
        .iter()
        .filter_map(|&i| items.get(i))
        .map(|item| item.bounding_rect());
    let first = rects.next()?;
    Some(rects.fold(first, |acc, r| acc.union(r)))
}

fn screen_to_scene(ui: &Ui, screen_pos: Pos2) -> Pos2 {
    let layer_id = ui.layer_id();
    if let Some(from_global) = ui.ctx().layer_transform_from_global(layer_id) {
        from_global * screen_pos
    } else {
        screen_pos
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_scene(
    ui: &mut Ui,
    items: &mut Vec<BoardItem>,
    selected: &mut HashSet<usize>,
    interaction: &mut InteractionState,
    undo_stack: &mut UndoStack,
    show_grid: bool,
    grid_size: f32,
    snap_to_grid: bool,
    visible_rect: Rect,
    next_texture_id: &mut u64,
    pending_export: &mut Option<Vec<u8>>,
) {
    let pointer_pos = ui
        .ctx()
        .pointer_interact_pos()
        .map(|p| screen_to_scene(ui, p));
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let primary_pressed = ui.input(|i| i.pointer.primary_pressed());
    let primary_released = ui.input(|i| i.pointer.primary_released());
    let shift_held = ui.input(|i| i.modifiers.shift);
    let double_clicked = ui.input(|i| {
        i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
    });

    // Expanded visible rect for handle culling
    let cull_rect = visible_rect.expand(HANDLE_SIZE + ROT_HANDLE_OFFSET);

    if show_grid {
        draw_grid(ui, visible_rect, grid_size);
    }

    // Lazy decode: decode up to N visible items per frame
    let mut decoded = 0;
    for item in items.iter_mut() {
        if decoded >= LAZY_DECODE_PER_FRAME {
            break;
        }
        if item.needs_decode() && item.bounding_rect().intersects(cull_rect) {
            *next_texture_id += 1;
            let name = format!("lazy-{}", next_texture_id);
            if item.ensure_texture(ui.ctx(), &name) {
                decoded += 1;
            }
        }
    }

    // Allocate rects for hit testing
    let mut hovered_item: Option<usize> = None;
    let mut item_responses: Vec<(usize, egui::Response)> = Vec::with_capacity(items.len());

    for (idx, item) in items.iter().enumerate() {
        let rect = item.bounding_rect();
        if !rect.intersects(cull_rect) {
            continue;
        }
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        item_responses.push((idx, resp));
    }

    // Topmost hovered (last in z-order = drawn on top)
    for (idx, resp) in item_responses.iter().rev() {
        if resp.hovered() {
            hovered_item = Some(*idx);
            break;
        }
    }

    // Label hit testing (labels render on top of images)
    let mut hovered_label: Option<(usize, usize)> = None;
    if let Some(pointer) = pointer_pos {
        for (item_idx, item) in items.iter().enumerate().rev() {
            if !item.bounding_rect().intersects(cull_rect) {
                continue;
            }
            let image_pos = item.transform().position;
            for (label_idx, label) in item.labels().iter().enumerate().rev() {
                if label.bounding_rect(image_pos).contains(pointer) {
                    hovered_label = Some((item_idx, label_idx));
                    break;
                }
            }
            if hovered_label.is_some() {
                break;
            }
        }
    }

    // Draw items (with frustum culling)
    for &(idx, _) in &item_responses {
        draw_item(ui, &items[idx], selected.contains(&idx));
    }

    // Draw selection handles, detect handle hover
    let handle_hit = draw_selection_handles(ui, items, selected, pointer_pos);

    if (hovered_item.is_some() || hovered_label.is_some()) && handle_hit.is_none() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // State machine
    let current_state = std::mem::take(interaction);
    *interaction = match current_state {
        InteractionState::Idle => handle_idle(
            primary_pressed,
            double_clicked,
            shift_held,
            pointer_pos,
            &handle_hit,
            hovered_item,
            hovered_label,
            items,
            selected,
            undo_stack,
        ),
        InteractionState::DraggingItems {
            drag_started,
            last_pointer,
            start_positions,
        } => handle_dragging(
            primary_released,
            primary_down,
            pointer_pos,
            drag_started,
            last_pointer,
            start_positions,
            items,
            undo_stack,
            snap_to_grid,
            grid_size,
        ),
        InteractionState::SelectionRect { start } => handle_selection_rect(
            ui,
            primary_released,
            shift_held,
            pointer_pos,
            start,
            items,
            selected,
        ),
        InteractionState::ResizingHandle {
            corner,
            start_mouse,
            initial_scales,
            initial_positions,
            group_rect,
        } => handle_resizing(
            primary_released,
            pointer_pos,
            corner,
            start_mouse,
            initial_scales,
            initial_positions,
            group_rect,
            items,
            undo_stack,
        ),
        InteractionState::RotatingHandle {
            center,
            start_angle,
            initial_rotations,
            initial_positions,
        } => handle_rotating(
            primary_released,
            pointer_pos,
            center,
            start_angle,
            initial_rotations,
            initial_positions,
            items,
            undo_stack,
        ),
        InteractionState::Cropping {
            idx,
            start,
            current,
        } => handle_cropping(
            ui,
            primary_pressed,
            primary_released,
            primary_down,
            pointer_pos,
            idx,
            start,
            current,
            items,
            undo_stack,
        ),
        InteractionState::EditingText { idx } => handle_editing_text(ui, idx, items, undo_stack),
        InteractionState::DraggingLabel {
            item_idx,
            label_idx,
            start_offset,
            last_pointer,
        } => handle_dragging_label(
            primary_released,
            pointer_pos,
            item_idx,
            label_idx,
            start_offset,
            last_pointer,
            items,
            undo_stack,
        ),
        InteractionState::EditingLabel {
            item_idx,
            label_idx,
        } => handle_editing_label(ui, item_idx, label_idx, items, undo_stack),
        InteractionState::ExportingRegion { start, current } => handle_export_region(
            ui,
            primary_pressed,
            primary_released,
            primary_down,
            pointer_pos,
            start,
            current,
            items,
            pending_export,
        ),
    };
}

#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
fn handle_idle(
    primary_pressed: bool,
    double_clicked: bool,
    shift_held: bool,
    pointer_pos: Option<Pos2>,
    handle_hit: &Option<HandleHit>,
    hovered_item: Option<usize>,
    hovered_label: Option<(usize, usize)>,
    items: &mut Vec<BoardItem>,
    selected: &mut HashSet<usize>,
    undo_stack: &mut UndoStack,
) -> InteractionState {
    // Double-click on label: edit label
    if double_clicked && let Some((item_idx, label_idx)) = hovered_label {
        selected.clear();
        selected.insert(item_idx);
        return InteractionState::EditingLabel {
            item_idx,
            label_idx,
        };
    }

    // Single click on label: start dragging
    if primary_pressed
        && !double_clicked
        && let Some((item_idx, label_idx)) = hovered_label
        && let Some(pointer) = pointer_pos
    {
        let start_offset = items
            .get(item_idx)
            .and_then(|item| item.labels().get(label_idx))
            .map(|l| l.offset)
            .unwrap_or(Vec2::ZERO);
        selected.clear();
        selected.insert(item_idx);
        return InteractionState::DraggingLabel {
            item_idx,
            label_idx,
            start_offset,
            last_pointer: pointer,
        };
    }

    // Double-click: edit text or create new text
    if double_clicked && let Some(pointer) = pointer_pos {
        if let Some(idx) = hovered_item {
            if items.get(idx).is_some_and(|i| i.text_content().is_some()) {
                selected.clear();
                selected.insert(idx);
                return InteractionState::EditingText { idx };
            }
        } else {
            // Double-click on empty canvas — create new text
            let pos = pointer.to_vec2();
            items.push(BoardItem::new_text("Text".into(), pos));
            let idx = items.len() - 1;
            undo_stack.push(Command::Add { count: 1 });
            selected.clear();
            selected.insert(idx);
            return InteractionState::EditingText { idx };
        }
    }

    if !primary_pressed {
        return InteractionState::Idle;
    }
    let Some(pointer) = pointer_pos else {
        return InteractionState::Idle;
    };

    // Handle hit takes priority
    if let Some(hit) = handle_hit {
        match hit {
            HandleHit::Resize(corner) => {
                return start_resize(pointer, *corner, items, selected);
            }
            HandleHit::Rotate => {
                return start_rotate(pointer, items, selected);
            }
        }
    }

    // Item click
    if let Some(idx) = hovered_item {
        if shift_held {
            if selected.contains(&idx) {
                selected.remove(&idx);
            } else {
                selected.insert(idx);
            }
        } else if !selected.contains(&idx) {
            selected.clear();
            selected.insert(idx);
        }
        let start_positions: Vec<(usize, Vec2)> = selected
            .iter()
            .filter_map(|&i| items.get(i).map(|item| (i, item.transform().position)))
            .collect();
        return InteractionState::DraggingItems {
            drag_started: false,
            last_pointer: pointer,
            start_positions,
        };
    }

    // Empty canvas click
    if !shift_held {
        selected.clear();
    }
    InteractionState::SelectionRect { start: pointer }
}

#[allow(clippy::too_many_arguments)]
fn handle_dragging(
    primary_released: bool,
    primary_down: bool,
    pointer_pos: Option<Pos2>,
    mut drag_started: bool,
    last_pointer: Pos2,
    start_positions: Vec<(usize, Vec2)>,
    items: &mut [BoardItem],
    undo_stack: &mut UndoStack,
    snap_to_grid: bool,
    grid_size: f32,
) -> InteractionState {
    if primary_released {
        // Snap to grid on release
        if snap_to_grid && drag_started {
            let gs = grid_size;
            for (idx, _) in &start_positions {
                if let Some(item) = items.get_mut(*idx) {
                    let pos = &mut item.transform_mut().position;
                    pos.x = (pos.x / gs).round() * gs;
                    pos.y = (pos.y / gs).round() * gs;
                }
            }
        }

        if drag_started {
            let indices: Vec<usize> = start_positions.iter().map(|(i, _)| *i).collect();
            if let Some((first_idx, first_start)) = start_positions.first()
                && let Some(item) = items.get(*first_idx)
            {
                let delta = item.transform().position - *first_start;
                if delta.length_sq() > 0.01 {
                    undo_stack.push(Command::Move { indices, delta });
                }
            }
        }
        return InteractionState::Idle;
    }

    let mut current_pointer = last_pointer;
    if primary_down && let Some(mouse) = pointer_pos {
        let drag_delta = mouse - last_pointer;
        if drag_delta.length_sq() > 0.0 {
            drag_started = true;
            let sel: Vec<usize> = start_positions.iter().map(|(i, _)| *i).collect();
            for idx in sel {
                if let Some(item) = items.get_mut(idx) {
                    item.transform_mut().position += drag_delta;
                }
            }
            current_pointer = mouse;
        }
    }

    InteractionState::DraggingItems {
        drag_started,
        last_pointer: current_pointer,
        start_positions,
    }
}

fn handle_selection_rect(
    ui: &mut Ui,
    primary_released: bool,
    shift_held: bool,
    pointer_pos: Option<Pos2>,
    start: Pos2,
    items: &[BoardItem],
    selected: &mut HashSet<usize>,
) -> InteractionState {
    if primary_released {
        if let Some(end) = pointer_pos {
            let sel_rect = Rect::from_two_pos(start, end);
            if sel_rect.area() > 4.0 {
                if !shift_held {
                    selected.clear();
                }
                for (idx, item) in items.iter().enumerate() {
                    if sel_rect.intersects(item.bounding_rect()) {
                        selected.insert(idx);
                    }
                }
            }
        }
        return InteractionState::Idle;
    }

    if let Some(end) = pointer_pos {
        let sel_rect = Rect::from_two_pos(start, end);
        ui.painter().rect(
            sel_rect,
            0.0,
            Color32::from_rgba_premultiplied(100, 160, 255, 30),
            Stroke::new(1.0, SELECTION_COLOR),
            StrokeKind::Outside,
        );
    }

    InteractionState::SelectionRect { start }
}

fn start_resize(
    mouse: Pos2,
    corner: Corner,
    items: &[BoardItem],
    selected: &HashSet<usize>,
) -> InteractionState {
    let initial_scales: Vec<(usize, Vec2)> = selected
        .iter()
        .filter_map(|&i| items.get(i).map(|item| (i, item.transform().scale)))
        .collect();
    let initial_positions: Vec<(usize, Vec2)> = selected
        .iter()
        .filter_map(|&i| items.get(i).map(|item| (i, item.transform().position)))
        .collect();
    let Some(group_rect) = selected_bounding_rect(items, selected) else {
        return InteractionState::Idle;
    };

    InteractionState::ResizingHandle {
        corner,
        start_mouse: mouse,
        initial_scales,
        initial_positions,
        group_rect,
    }
}

fn start_rotate(mouse: Pos2, items: &[BoardItem], selected: &HashSet<usize>) -> InteractionState {
    let Some(group_rect) = selected_bounding_rect(items, selected) else {
        return InteractionState::Idle;
    };
    let center = group_rect.center();
    let start_angle = (mouse - center).angle();

    let initial_rotations: Vec<(usize, f32)> = selected
        .iter()
        .filter_map(|&i| items.get(i).map(|item| (i, item.transform().rotation)))
        .collect();
    let initial_positions: Vec<(usize, Vec2)> = selected
        .iter()
        .filter_map(|&i| items.get(i).map(|item| (i, item.transform().position)))
        .collect();

    InteractionState::RotatingHandle {
        center,
        start_angle,
        initial_rotations,
        initial_positions,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_resizing(
    primary_released: bool,
    pointer_pos: Option<Pos2>,
    corner: Corner,
    start_mouse: Pos2,
    initial_scales: Vec<(usize, Vec2)>,
    initial_positions: Vec<(usize, Vec2)>,
    group_rect: Rect,
    items: &mut [BoardItem],
    undo_stack: &mut UndoStack,
) -> InteractionState {
    if primary_released {
        let indices: Vec<usize> = initial_scales.iter().map(|(i, _)| *i).collect();
        let old_scales: Vec<Vec2> = initial_scales.iter().map(|(_, s)| *s).collect();
        let old_positions: Vec<Vec2> = initial_positions.iter().map(|(_, p)| *p).collect();
        let new_scales: Vec<Vec2> = indices
            .iter()
            .filter_map(|&i| items.get(i).map(|item| item.transform().scale))
            .collect();
        let new_positions: Vec<Vec2> = indices
            .iter()
            .filter_map(|&i| items.get(i).map(|item| item.transform().position))
            .collect();
        undo_stack.push(Command::Resize {
            indices,
            old_scales,
            new_scales,
            old_positions,
            new_positions,
        });
        return InteractionState::Idle;
    }

    if let Some(mouse) = pointer_pos {
        let anchor = corner.opposite().pos_in_rect(group_rect);
        let group_size = group_rect.size();

        if group_size.x >= 1.0 && group_size.y >= 1.0 {
            let initial_dist = (start_mouse - anchor).length();
            let current_dist = (mouse - anchor).length();
            if initial_dist >= 1.0 {
                let scale_factor = (current_dist / initial_dist).max(0.05);

                for (i, (idx, orig_scale)) in initial_scales.iter().enumerate() {
                    if let Some(item) = items.get_mut(*idx) {
                        let t = item.transform_mut();
                        t.scale = *orig_scale * scale_factor;

                        if let Some((_, orig_pos)) = initial_positions.get(i) {
                            let offset = orig_pos.to_pos2() - anchor;
                            t.position = (anchor + offset * scale_factor).to_vec2();
                        }
                    }
                }
            }
        }
    }

    InteractionState::ResizingHandle {
        corner,
        start_mouse,
        initial_scales,
        initial_positions,
        group_rect,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_rotating(
    primary_released: bool,
    pointer_pos: Option<Pos2>,
    center: Pos2,
    start_angle: f32,
    initial_rotations: Vec<(usize, f32)>,
    initial_positions: Vec<(usize, Vec2)>,
    items: &mut [BoardItem],
    undo_stack: &mut UndoStack,
) -> InteractionState {
    if primary_released {
        let indices: Vec<usize> = initial_rotations.iter().map(|(i, _)| *i).collect();
        let old_rotations: Vec<f32> = initial_rotations.iter().map(|(_, r)| *r).collect();
        let old_positions: Vec<Vec2> = initial_positions.iter().map(|(_, p)| *p).collect();
        let new_rotations: Vec<f32> = indices
            .iter()
            .filter_map(|&i| items.get(i).map(|item| item.transform().rotation))
            .collect();
        let new_positions: Vec<Vec2> = indices
            .iter()
            .filter_map(|&i| items.get(i).map(|item| item.transform().position))
            .collect();
        undo_stack.push(Command::Rotate {
            indices,
            old_rotations,
            new_rotations,
            old_positions,
            new_positions,
        });
        return InteractionState::Idle;
    }

    if let Some(mouse) = pointer_pos {
        let current_angle = (mouse - center).angle();
        let delta_angle = current_angle - start_angle;

        for (i, (idx, orig_rot)) in initial_rotations.iter().enumerate() {
            if let Some(item) = items.get_mut(*idx) {
                let t = item.transform_mut();
                t.rotation = orig_rot + delta_angle;

                if let Some((_, orig_pos)) = initial_positions.get(i) {
                    let offset = orig_pos.to_pos2() - center;
                    let cos = delta_angle.cos();
                    let sin = delta_angle.sin();
                    let rotated = Vec2::new(
                        offset.x * cos - offset.y * sin,
                        offset.x * sin + offset.y * cos,
                    );
                    t.position = (center + rotated).to_vec2();
                }
            }
        }
    }

    InteractionState::RotatingHandle {
        center,
        start_angle,
        initial_rotations,
        initial_positions,
    }
}

#[allow(clippy::ptr_arg)]
fn handle_editing_text(
    ui: &mut Ui,
    idx: usize,
    items: &mut Vec<BoardItem>,
    undo_stack: &mut UndoStack,
) -> InteractionState {
    let Some(item) = items.get(idx) else {
        return InteractionState::Idle;
    };

    let old_content = item.text_content().unwrap_or("").to_string();
    let pos = item.transform().position;
    let BoardItem::Text(txt) = item else {
        return InteractionState::Idle;
    };
    let font_size = txt.font_size;

    let text_edit_id = egui::Id::new("text_edit_inline").with(idx);
    let mut text = items[idx].text_content().unwrap_or("").to_string();

    let rect = items[idx].bounding_rect();
    let resp = ui.put(
        Rect::from_min_size(
            pos.to_pos2(),
            Vec2::new(rect.width().max(100.0), font_size * 2.0),
        ),
        egui::TextEdit::singleline(&mut text)
            .id(text_edit_id)
            .font(egui::FontId::proportional(font_size))
            .desired_width(200.0),
    );

    items[idx].set_text_content(text.clone());

    if !resp.has_focus() {
        resp.request_focus();
    }

    let enter = ui.input(|i| i.key_pressed(Key::Enter));
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    if enter || escape || resp.lost_focus() {
        if escape {
            items[idx].set_text_content(old_content);
        } else if text != old_content {
            undo_stack.push(Command::EditText {
                idx,
                old_content,
                new_content: text,
            });
        }
        if items[idx].text_content().is_some_and(|t| t.is_empty()) {
            items.remove(idx);
        }
        return InteractionState::Idle;
    }

    InteractionState::EditingText { idx }
}

#[allow(clippy::too_many_arguments)]
fn handle_dragging_label(
    primary_released: bool,
    pointer_pos: Option<Pos2>,
    item_idx: usize,
    label_idx: usize,
    start_offset: Vec2,
    last_pointer: Pos2,
    items: &mut [BoardItem],
    undo_stack: &mut UndoStack,
) -> InteractionState {
    if primary_released {
        let current_offset = items
            .get(item_idx)
            .and_then(|item| item.labels().get(label_idx))
            .map(|l| l.offset)
            .unwrap_or(start_offset);
        if (current_offset - start_offset).length_sq() > 0.01 {
            undo_stack.push(Command::MoveLabel {
                item_idx,
                label_idx,
                old_offset: start_offset,
                new_offset: current_offset,
            });
        }
        return InteractionState::Idle;
    }

    if let Some(mouse) = pointer_pos {
        let delta = mouse - last_pointer;
        if delta.length_sq() > 0.0 {
            if let Some(item) = items.get_mut(item_idx)
                && let Some(labels) = item.labels_mut()
                && let Some(label) = labels.get_mut(label_idx)
            {
                label.offset += delta;
            }
            return InteractionState::DraggingLabel {
                item_idx,
                label_idx,
                start_offset,
                last_pointer: mouse,
            };
        }
    }

    InteractionState::DraggingLabel {
        item_idx,
        label_idx,
        start_offset,
        last_pointer,
    }
}

#[allow(clippy::ptr_arg)]
fn handle_editing_label(
    ui: &mut Ui,
    item_idx: usize,
    label_idx: usize,
    items: &mut Vec<BoardItem>,
    undo_stack: &mut UndoStack,
) -> InteractionState {
    let Some(item) = items.get(item_idx) else {
        return InteractionState::Idle;
    };
    let image_pos = item.transform().position;
    let Some(label) = item.labels().get(label_idx) else {
        return InteractionState::Idle;
    };
    let old_text = label.text.clone();
    let label_pos = image_pos + label.offset;
    let font_size = label.font_size;

    let text_edit_id = egui::Id::new("label_edit_inline")
        .with(item_idx)
        .with(label_idx);
    let mut text = old_text.clone();

    let resp = ui.put(
        Rect::from_min_size(label_pos.to_pos2(), Vec2::new(200.0, font_size * 2.0)),
        egui::TextEdit::singleline(&mut text)
            .id(text_edit_id)
            .font(egui::FontId::proportional(font_size))
            .desired_width(200.0),
    );

    if let Some(item) = items.get_mut(item_idx)
        && let Some(labels) = item.labels_mut()
        && let Some(label) = labels.get_mut(label_idx)
    {
        label.text = text.clone();
    }

    if !resp.has_focus() {
        resp.request_focus();
    }

    let enter = ui.input(|i| i.key_pressed(Key::Enter));
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    if enter || escape || resp.lost_focus() {
        if escape {
            if let Some(item) = items.get_mut(item_idx)
                && let Some(labels) = item.labels_mut()
                && let Some(label) = labels.get_mut(label_idx)
            {
                label.text = old_text;
            }
        } else if text != old_text {
            undo_stack.push(Command::EditLabel {
                item_idx,
                label_idx,
                old_text,
                new_text: text.clone(),
            });
        }
        // Remove empty labels
        let is_empty = items
            .get(item_idx)
            .and_then(|item| item.labels().get(label_idx))
            .is_some_and(|l| l.text.is_empty());
        if is_empty
            && let Some(item) = items.get_mut(item_idx)
            && let Some(labels) = item.labels_mut()
        {
            let label = labels.remove(label_idx);
            undo_stack.push(Command::DeleteLabel {
                item_idx,
                label_idx,
                label,
            });
        }
        return InteractionState::Idle;
    }

    InteractionState::EditingLabel {
        item_idx,
        label_idx,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_cropping(
    ui: &mut Ui,
    primary_pressed: bool,
    primary_released: bool,
    primary_down: bool,
    pointer_pos: Option<Pos2>,
    idx: usize,
    start: Option<Pos2>,
    current: Pos2,
    items: &mut [BoardItem],
    undo_stack: &mut UndoStack,
) -> InteractionState {
    let Some(item) = items.get(idx) else {
        return InteractionState::Idle;
    };
    let item_rect = item.bounding_rect();
    let Some(orig_size) = item.original_size() else {
        return InteractionState::Idle;
    };

    if ui.input(|i| i.key_pressed(Key::Escape)) {
        return InteractionState::Idle;
    }

    let overlay_color = Color32::from_rgba_premultiplied(0, 0, 0, 100);
    ui.painter().rect_filled(item_rect, 0.0, overlay_color);

    // Waiting for first click
    if start.is_none() && !primary_pressed {
        ui.painter().text(
            item_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Click and drag to crop",
            egui::FontId::proportional(14.0),
            Color32::WHITE,
        );
        return InteractionState::Cropping {
            idx,
            start: None,
            current: Pos2::ZERO,
        };
    }

    // First click — begin crop rectangle
    if start.is_none() && primary_pressed {
        if let Some(mouse) = pointer_pos {
            let clamped = Pos2::new(
                mouse.x.clamp(item_rect.min.x, item_rect.max.x),
                mouse.y.clamp(item_rect.min.y, item_rect.max.y),
            );
            return InteractionState::Cropping {
                idx,
                start: Some(clamped),
                current: clamped,
            };
        }
        return InteractionState::Cropping {
            idx,
            start,
            current,
        };
    }

    let start = start.expect("start should be Some at this point");

    // Dragging — update current
    let mut cur = current;
    if primary_down && let Some(mouse) = pointer_pos {
        cur = Pos2::new(
            mouse.x.clamp(item_rect.min.x, item_rect.max.x),
            mouse.y.clamp(item_rect.min.y, item_rect.max.y),
        );
    }

    // Draw crop rectangle preview
    let crop_screen_rect = Rect::from_two_pos(start, cur);
    if crop_screen_rect.area() > 4.0 {
        ui.painter().rect_filled(
            crop_screen_rect,
            0.0,
            Color32::from_rgba_premultiplied(0, 0, 0, 0),
        );
        ui.painter().rect_stroke(
            crop_screen_rect,
            0.0,
            Stroke::new(2.0, Color32::from_rgb(255, 200, 50)),
            StrokeKind::Outside,
        );
    }

    // Release — confirm crop
    if primary_released && crop_screen_rect.area() > 4.0 {
        let Some(item) = items.get(idx) else {
            return InteractionState::Idle;
        };
        let scale = item.transform().scale;
        let pos = item.transform().position;
        let crop_in_pixels = Rect::from_min_max(
            egui::pos2(
                (crop_screen_rect.min.x - pos.x) / scale.x,
                (crop_screen_rect.min.y - pos.y) / scale.y,
            ),
            egui::pos2(
                (crop_screen_rect.max.x - pos.x) / scale.x,
                (crop_screen_rect.max.y - pos.y) / scale.y,
            ),
        );

        let clamped = Rect::from_min_max(
            egui::pos2(
                crop_in_pixels.min.x.max(0.0).min(orig_size.x),
                crop_in_pixels.min.y.max(0.0).min(orig_size.y),
            ),
            egui::pos2(
                crop_in_pixels.max.x.max(0.0).min(orig_size.x),
                crop_in_pixels.max.y.max(0.0).min(orig_size.y),
            ),
        );

        let old_rect = items[idx].crop_rect();
        items[idx].set_crop_rect(Some(clamped));
        undo_stack.push(Command::Crop {
            idx,
            old_rect,
            new_rect: Some(clamped),
        });
        return InteractionState::Idle;
    }

    if primary_released {
        return InteractionState::Idle;
    }

    InteractionState::Cropping {
        idx,
        start: Some(start),
        current: cur,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_export_region(
    ui: &mut Ui,
    primary_pressed: bool,
    primary_released: bool,
    primary_down: bool,
    pointer_pos: Option<Pos2>,
    start: Option<Pos2>,
    current: Pos2,
    items: &[BoardItem],
    pending_export: &mut Option<Vec<u8>>,
) -> InteractionState {
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        return InteractionState::Idle;
    }

    // Hint text
    if start.is_none() && !primary_pressed {
        if let Some(pointer) = pointer_pos {
            ui.painter().text(
                pointer + Vec2::new(15.0, 15.0),
                egui::Align2::LEFT_TOP,
                "Drag to select export region",
                egui::FontId::proportional(14.0),
                Color32::from_rgb(255, 200, 50),
            );
        }
        return InteractionState::ExportingRegion {
            start: None,
            current: Pos2::ZERO,
        };
    }

    // First click
    if start.is_none() && primary_pressed {
        if let Some(mouse) = pointer_pos {
            return InteractionState::ExportingRegion {
                start: Some(mouse),
                current: mouse,
            };
        }
        return InteractionState::ExportingRegion { start, current };
    }

    let start = start.expect("start should be Some at this point");

    // Dragging
    let mut cur = current;
    if primary_down && let Some(mouse) = pointer_pos {
        cur = mouse;
    }

    // Draw export rectangle
    let export_rect = Rect::from_two_pos(start, cur);
    if export_rect.area() > 4.0 {
        ui.painter().rect(
            export_rect,
            0.0,
            Color32::from_rgba_premultiplied(255, 200, 50, 20),
            Stroke::new(2.0, Color32::from_rgb(255, 200, 50)),
            StrokeKind::Outside,
        );
    }

    // Release — render export, defer file dialog to update()
    if primary_released && export_rect.area() > 4.0 {
        let region_items: Vec<&BoardItem> = items
            .iter()
            .filter(|item| item.bounding_rect().intersects(export_rect))
            .collect();

        if !region_items.is_empty() {
            match clipboard::render_region(&region_items, export_rect) {
                Ok(png) => *pending_export = Some(png),
                Err(e) => log::error!("Export render failed: {e}"),
            }
        }
        return InteractionState::Idle;
    }

    if primary_released {
        return InteractionState::Idle;
    }

    InteractionState::ExportingRegion {
        start: Some(start),
        current: cur,
    }
}
