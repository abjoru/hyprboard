use std::collections::HashSet;

use egui::{Color32, Rect, Vec2};

use crate::items::{BoardItem, Label};

#[derive(Clone)]
pub enum Command {
    Move {
        indices: Vec<usize>,
        delta: Vec2,
    },
    Resize {
        indices: Vec<usize>,
        old_scales: Vec<Vec2>,
        new_scales: Vec<Vec2>,
        old_positions: Vec<Vec2>,
        new_positions: Vec<Vec2>,
    },
    Rotate {
        indices: Vec<usize>,
        old_rotations: Vec<f32>,
        new_rotations: Vec<f32>,
        old_positions: Vec<Vec2>,
        new_positions: Vec<Vec2>,
    },
    Delete {
        items: Vec<(usize, BoardItem)>,
    },
    Add {
        count: usize,
    },
    ZOrder {
        old_order: Vec<usize>,
    },
    Flip {
        indices: Vec<usize>,
        horizontal: bool,
    },
    Grayscale {
        indices: Vec<usize>,
    },
    Opacity {
        indices: Vec<usize>,
        old_values: Vec<f32>,
        new_values: Vec<f32>,
    },
    Crop {
        idx: usize,
        old_rect: Option<Rect>,
        new_rect: Option<Rect>,
    },
    EditText {
        idx: usize,
        old_content: String,
        new_content: String,
    },
    AddLabel {
        item_idx: usize,
    },
    DeleteLabel {
        item_idx: usize,
        label_idx: usize,
        label: Label,
    },
    MoveLabel {
        item_idx: usize,
        label_idx: usize,
        old_offset: Vec2,
        new_offset: Vec2,
    },
    EditLabel {
        item_idx: usize,
        label_idx: usize,
        old_text: String,
        new_text: String,
    },
    TextStyle {
        indices: Vec<usize>,
        old_font_sizes: Vec<f32>,
        new_font_sizes: Vec<f32>,
        old_colors: Vec<Color32>,
        new_colors: Vec<Color32>,
        old_bg_colors: Vec<Color32>,
        new_bg_colors: Vec<Color32>,
    },
    BorderColor {
        indices: Vec<usize>,
        old_colors: Vec<Color32>,
        new_colors: Vec<Color32>,
    },
}

#[derive(Default)]
pub struct UndoStack {
    undos: Vec<Command>,
    redos: Vec<Command>,
    pub dirty: bool,
}

impl UndoStack {
    pub fn push(&mut self, cmd: Command) {
        self.undos.push(cmd);
        self.redos.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self, items: &mut Vec<BoardItem>, selected: &mut HashSet<usize>) {
        if let Some(cmd) = self.undos.pop() {
            let reverse = Self::apply_reverse(&cmd, items, selected);
            self.redos.push(reverse);
            self.dirty = true;
        }
    }

    pub fn redo(&mut self, items: &mut Vec<BoardItem>, selected: &mut HashSet<usize>) {
        if let Some(cmd) = self.redos.pop() {
            let reverse = Self::apply_reverse(&cmd, items, selected);
            self.undos.push(reverse);
            self.dirty = true;
        }
    }

    fn apply_reverse(
        cmd: &Command,
        items: &mut Vec<BoardItem>,
        selected: &mut HashSet<usize>,
    ) -> Command {
        match cmd {
            Command::Move { indices, delta } => {
                for &idx in indices {
                    if let Some(item) = items.get_mut(idx) {
                        item.transform_mut().position -= *delta;
                    }
                }
                Command::Move {
                    indices: indices.clone(),
                    delta: -*delta,
                }
            }
            Command::Resize {
                indices,
                old_scales,
                new_scales,
                old_positions,
                new_positions,
            } => {
                for (i, &idx) in indices.iter().enumerate() {
                    if let Some(item) = items.get_mut(idx) {
                        let t = item.transform_mut();
                        t.scale = old_scales[i];
                        t.position = old_positions[i];
                    }
                }
                Command::Resize {
                    indices: indices.clone(),
                    old_scales: new_scales.clone(),
                    new_scales: old_scales.clone(),
                    old_positions: new_positions.clone(),
                    new_positions: old_positions.clone(),
                }
            }
            Command::Rotate {
                indices,
                old_rotations,
                new_rotations,
                old_positions,
                new_positions,
            } => {
                for (i, &idx) in indices.iter().enumerate() {
                    if let Some(item) = items.get_mut(idx) {
                        let t = item.transform_mut();
                        t.rotation = old_rotations[i];
                        t.position = old_positions[i];
                    }
                }
                Command::Rotate {
                    indices: indices.clone(),
                    old_rotations: new_rotations.clone(),
                    new_rotations: old_rotations.clone(),
                    old_positions: new_positions.clone(),
                    new_positions: old_positions.clone(),
                }
            }
            Command::Delete { items: deleted } => {
                let mut sorted = deleted.clone();
                sorted.sort_by_key(|(idx, _)| *idx);
                selected.clear();
                for (idx, item) in sorted {
                    items.insert(idx, item);
                    selected.insert(idx);
                }
                Command::Add {
                    count: deleted.len(),
                }
            }
            Command::Add { count } => {
                let start = items.len().saturating_sub(*count);
                let removed: Vec<_> = (start..items.len()).zip(items.drain(start..)).collect();
                selected.clear();
                Command::Delete { items: removed }
            }
            Command::ZOrder { old_order } => {
                let mut reordered = Vec::with_capacity(items.len());
                for &idx in old_order {
                    if let Some(item) = items.get(idx) {
                        reordered.push(item.clone());
                    }
                }
                let current_len = items.len();
                *items = reordered;
                selected.clear();
                // Build inverse permutation
                let mut inv = vec![0usize; current_len];
                for (new_pos, &old_pos) in old_order.iter().enumerate() {
                    if old_pos < inv.len() {
                        inv[old_pos] = new_pos;
                    }
                }
                Command::ZOrder { old_order: inv }
            }
            Command::Flip {
                indices,
                horizontal,
            } => {
                for &idx in indices {
                    if let Some(item) = items.get_mut(idx) {
                        item.toggle_flip(*horizontal);
                    }
                }
                Command::Flip {
                    indices: indices.clone(),
                    horizontal: *horizontal,
                }
            }
            Command::Grayscale { indices } => {
                for &idx in indices {
                    if let Some(item) = items.get_mut(idx) {
                        item.toggle_grayscale();
                    }
                }
                Command::Grayscale {
                    indices: indices.clone(),
                }
            }
            Command::Opacity {
                indices,
                old_values,
                new_values,
            } => {
                for (i, &idx) in indices.iter().enumerate() {
                    if let Some(item) = items.get_mut(idx) {
                        item.set_opacity(old_values[i]);
                    }
                }
                Command::Opacity {
                    indices: indices.clone(),
                    old_values: new_values.clone(),
                    new_values: old_values.clone(),
                }
            }
            Command::Crop {
                idx,
                old_rect,
                new_rect,
            } => {
                if let Some(item) = items.get_mut(*idx) {
                    item.set_crop_rect(*old_rect);
                }
                Command::Crop {
                    idx: *idx,
                    old_rect: *new_rect,
                    new_rect: *old_rect,
                }
            }
            Command::EditText {
                idx,
                old_content,
                new_content,
            } => {
                if let Some(item) = items.get_mut(*idx) {
                    item.set_text_content(old_content.clone());
                }
                Command::EditText {
                    idx: *idx,
                    old_content: new_content.clone(),
                    new_content: old_content.clone(),
                }
            }
            Command::AddLabel { item_idx } => {
                let Some(item) = items.get_mut(*item_idx) else {
                    return Command::AddLabel {
                        item_idx: *item_idx,
                    };
                };
                let Some(labels) = item.labels_mut() else {
                    return Command::AddLabel {
                        item_idx: *item_idx,
                    };
                };
                let Some(label) = labels.pop() else {
                    return Command::AddLabel {
                        item_idx: *item_idx,
                    };
                };
                let label_count = items[*item_idx].labels().len();
                Command::DeleteLabel {
                    item_idx: *item_idx,
                    label_idx: label_count,
                    label,
                }
            }
            Command::DeleteLabel {
                item_idx,
                label_idx,
                label,
            } => {
                if let Some(item) = items.get_mut(*item_idx)
                    && let Some(labels) = item.labels_mut()
                {
                    labels.insert(*label_idx, label.clone());
                }
                Command::AddLabel {
                    item_idx: *item_idx,
                }
            }
            Command::MoveLabel {
                item_idx,
                label_idx,
                old_offset,
                new_offset,
            } => {
                if let Some(item) = items.get_mut(*item_idx)
                    && let Some(labels) = item.labels_mut()
                    && let Some(label) = labels.get_mut(*label_idx)
                {
                    label.offset = *old_offset;
                }
                Command::MoveLabel {
                    item_idx: *item_idx,
                    label_idx: *label_idx,
                    old_offset: *new_offset,
                    new_offset: *old_offset,
                }
            }
            Command::EditLabel {
                item_idx,
                label_idx,
                old_text,
                new_text,
            } => {
                if let Some(item) = items.get_mut(*item_idx)
                    && let Some(labels) = item.labels_mut()
                    && let Some(label) = labels.get_mut(*label_idx)
                {
                    label.text = old_text.clone();
                }
                Command::EditLabel {
                    item_idx: *item_idx,
                    label_idx: *label_idx,
                    old_text: new_text.clone(),
                    new_text: old_text.clone(),
                }
            }
            Command::BorderColor {
                indices,
                old_colors,
                new_colors,
            } => {
                for (i, &idx) in indices.iter().enumerate() {
                    if let Some(item) = items.get_mut(idx) {
                        item.set_border_color(old_colors[i]);
                    }
                }
                Command::BorderColor {
                    indices: indices.clone(),
                    old_colors: new_colors.clone(),
                    new_colors: old_colors.clone(),
                }
            }
            Command::TextStyle {
                indices,
                old_font_sizes,
                new_font_sizes,
                old_colors,
                new_colors,
                old_bg_colors,
                new_bg_colors,
            } => {
                for (i, &idx) in indices.iter().enumerate() {
                    if let Some(item) = items.get_mut(idx) {
                        item.set_text_font_size(old_font_sizes[i]);
                        item.set_text_color(old_colors[i]);
                        item.set_text_bg_color(old_bg_colors[i]);
                    }
                }
                Command::TextStyle {
                    indices: indices.clone(),
                    old_font_sizes: new_font_sizes.clone(),
                    new_font_sizes: old_font_sizes.clone(),
                    old_colors: new_colors.clone(),
                    new_colors: old_colors.clone(),
                    old_bg_colors: new_bg_colors.clone(),
                    new_bg_colors: old_bg_colors.clone(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::items::Transform;

    use super::*;

    fn make_test_items() -> Vec<BoardItem> {
        vec![
            BoardItem::new_text("A".into(), Vec2::new(0.0, 0.0)),
            BoardItem::new_text("B".into(), Vec2::new(100.0, 0.0)),
            BoardItem::new_text("C".into(), Vec2::new(200.0, 0.0)),
        ]
    }

    #[test]
    fn undo_move() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        // Move items 0 and 1 by (10, 20)
        let delta = Vec2::new(10.0, 20.0);
        items[0].transform_mut().position += delta;
        items[1].transform_mut().position += delta;
        stack.push(Command::Move {
            indices: vec![0, 1],
            delta,
        });

        assert_eq!(items[0].transform().position, Vec2::new(10.0, 20.0));
        assert_eq!(items[1].transform().position, Vec2::new(110.0, 20.0));

        // Undo
        stack.undo(&mut items, &mut selected);
        assert_eq!(items[0].transform().position, Vec2::new(0.0, 0.0));
        assert_eq!(items[1].transform().position, Vec2::new(100.0, 0.0));

        // Redo
        stack.redo(&mut items, &mut selected);
        assert_eq!(items[0].transform().position, Vec2::new(10.0, 20.0));
        assert_eq!(items[1].transform().position, Vec2::new(110.0, 20.0));
    }

    #[test]
    fn undo_delete() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        // Delete item 1 ("B")
        let removed = items.remove(1);
        stack.push(Command::Delete {
            items: vec![(1, removed)],
        });

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text_content().unwrap(), "A");
        assert_eq!(items[1].text_content().unwrap(), "C");

        // Undo — restore "B" at index 1
        stack.undo(&mut items, &mut selected);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].text_content().unwrap(), "B");
        assert!(selected.contains(&1));
    }

    #[test]
    fn undo_add() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        items.push(BoardItem::new_text("D".into(), Vec2::new(300.0, 0.0)));
        stack.push(Command::Add { count: 1 });

        assert_eq!(items.len(), 4);

        stack.undo(&mut items, &mut selected);
        assert_eq!(items.len(), 3);

        stack.redo(&mut items, &mut selected);
        assert_eq!(items.len(), 4);
        assert_eq!(items[3].text_content().unwrap(), "D");
    }

    #[test]
    fn undo_zorder() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        // Reverse order: [C, B, A]
        items.reverse();
        stack.push(Command::ZOrder {
            old_order: vec![2, 1, 0],
        });

        assert_eq!(items[0].text_content().unwrap(), "C");
        assert_eq!(items[2].text_content().unwrap(), "A");

        // Undo — restore original order
        stack.undo(&mut items, &mut selected);
        assert_eq!(items[0].text_content().unwrap(), "A");
        assert_eq!(items[1].text_content().unwrap(), "B");
        assert_eq!(items[2].text_content().unwrap(), "C");
    }

    #[test]
    fn undo_opacity() {
        let mut items = vec![BoardItem::new_text("X".into(), Vec2::ZERO)];
        // Text items have fixed 1.0 opacity, but we test the mechanism
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        stack.push(Command::Opacity {
            indices: vec![0],
            old_values: vec![1.0],
            new_values: vec![0.5],
        });

        stack.undo(&mut items, &mut selected);
        // Opacity on text items is a no-op, but command mechanics work
        assert!(stack.redos.len() == 1);
    }

    #[test]
    fn push_clears_redos() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        stack.push(Command::Move {
            indices: vec![0],
            delta: Vec2::new(5.0, 0.0),
        });
        items[0].transform_mut().position += Vec2::new(5.0, 0.0);

        stack.undo(&mut items, &mut selected);
        assert_eq!(stack.redos.len(), 1);

        // New push should clear redos
        stack.push(Command::Move {
            indices: vec![1],
            delta: Vec2::new(1.0, 0.0),
        });
        assert_eq!(stack.redos.len(), 0);
    }

    fn make_image_item(pos: Vec2, size: Vec2) -> BoardItem {
        BoardItem::Image(crate::items::ImageItem {
            texture: None,
            original_bytes: Arc::from(vec![0u8; 4]),
            original_size: size,
            transform: Transform::default().with_position(pos),
            crop_rect: None,
            opacity: 1.0,
            grayscale: false,
            flip_h: false,
            flip_v: false,
            labels: Vec::new(),
            border_color: Color32::TRANSPARENT,
        })
    }

    #[test]
    fn undo_resize() {
        let mut items = vec![make_image_item(
            Vec2::new(10.0, 10.0),
            Vec2::new(100.0, 100.0),
        )];
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        let old_scale = items[0].transform().scale;
        let old_pos = items[0].transform().position;
        items[0].transform_mut().scale = Vec2::splat(2.0);
        items[0].transform_mut().position = Vec2::new(5.0, 5.0);

        stack.push(Command::Resize {
            indices: vec![0],
            old_scales: vec![old_scale],
            new_scales: vec![Vec2::splat(2.0)],
            old_positions: vec![old_pos],
            new_positions: vec![Vec2::new(5.0, 5.0)],
        });

        stack.undo(&mut items, &mut selected);
        assert_eq!(items[0].transform().scale, Vec2::splat(1.0));
        assert_eq!(items[0].transform().position, Vec2::new(10.0, 10.0));

        stack.redo(&mut items, &mut selected);
        assert_eq!(items[0].transform().scale, Vec2::splat(2.0));
        assert_eq!(items[0].transform().position, Vec2::new(5.0, 5.0));
    }

    #[test]
    fn undo_rotate() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        items[0].transform_mut().rotation = 1.5;
        stack.push(Command::Rotate {
            indices: vec![0],
            old_rotations: vec![0.0],
            new_rotations: vec![1.5],
            old_positions: vec![Vec2::ZERO],
            new_positions: vec![Vec2::ZERO],
        });

        stack.undo(&mut items, &mut selected);
        assert!((items[0].transform().rotation).abs() < 0.001);

        stack.redo(&mut items, &mut selected);
        assert!((items[0].transform().rotation - 1.5).abs() < 0.001);
    }

    #[test]
    fn undo_crop() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        let crop = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(50.0, 50.0));
        items[0].set_crop_rect(Some(crop));
        stack.push(Command::Crop {
            idx: 0,
            old_rect: None,
            new_rect: Some(crop),
        });

        stack.undo(&mut items, &mut selected);
        assert!(items[0].crop_rect().is_none());

        stack.redo(&mut items, &mut selected);
        assert_eq!(items[0].crop_rect().unwrap(), crop);
    }

    #[test]
    fn undo_edit_text() {
        let mut items = vec![BoardItem::new_text("hello".into(), Vec2::ZERO)];
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        items[0].set_text_content("world".into());
        stack.push(Command::EditText {
            idx: 0,
            old_content: "hello".into(),
            new_content: "world".into(),
        });

        stack.undo(&mut items, &mut selected);
        assert_eq!(items[0].text_content().unwrap(), "hello");

        stack.redo(&mut items, &mut selected);
        assert_eq!(items[0].text_content().unwrap(), "world");
    }

    #[test]
    fn undo_flip() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        items[0].toggle_flip(true);
        stack.push(Command::Flip {
            indices: vec![0],
            horizontal: true,
        });

        assert!(matches!(&items[0], BoardItem::Image(img) if img.flip_h));

        stack.undo(&mut items, &mut selected);
        assert!(matches!(&items[0], BoardItem::Image(img) if !img.flip_h));

        stack.redo(&mut items, &mut selected);
        assert!(matches!(&items[0], BoardItem::Image(img) if img.flip_h));
    }

    #[test]
    fn undo_grayscale() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        // toggle_grayscale without texture just flips the flag
        items[0].toggle_grayscale();
        stack.push(Command::Grayscale { indices: vec![0] });

        assert!(matches!(&items[0], BoardItem::Image(img) if img.grayscale));

        stack.undo(&mut items, &mut selected);
        assert!(matches!(&items[0], BoardItem::Image(img) if !img.grayscale));
    }

    #[test]
    fn undo_add_label() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        items[0].add_label(Label::new("test".into(), Vec2::new(0.0, -20.0)));
        stack.push(Command::AddLabel { item_idx: 0 });

        assert_eq!(items[0].labels().len(), 1);

        stack.undo(&mut items, &mut selected);
        assert_eq!(items[0].labels().len(), 0);

        stack.redo(&mut items, &mut selected);
        assert_eq!(items[0].labels().len(), 1);
        assert_eq!(items[0].labels()[0].text, "test");
    }

    #[test]
    fn undo_delete_label() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        items[0].add_label(Label::new("keep".into(), Vec2::ZERO));
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        let label = items[0].labels()[0].clone();
        items[0].labels_mut().unwrap().remove(0);
        stack.push(Command::DeleteLabel {
            item_idx: 0,
            label_idx: 0,
            label,
        });

        assert_eq!(items[0].labels().len(), 0);

        stack.undo(&mut items, &mut selected);
        assert_eq!(items[0].labels().len(), 1);
        assert_eq!(items[0].labels()[0].text, "keep");
    }

    #[test]
    fn undo_move_label() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        items[0].add_label(Label::new("lbl".into(), Vec2::new(0.0, -20.0)));
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        let old_offset = Vec2::new(0.0, -20.0);
        let new_offset = Vec2::new(50.0, -30.0);
        items[0].labels_mut().unwrap()[0].offset = new_offset;
        stack.push(Command::MoveLabel {
            item_idx: 0,
            label_idx: 0,
            old_offset,
            new_offset,
        });

        stack.undo(&mut items, &mut selected);
        assert_eq!(items[0].labels()[0].offset, old_offset);

        stack.redo(&mut items, &mut selected);
        assert_eq!(items[0].labels()[0].offset, new_offset);
    }

    #[test]
    fn undo_edit_label() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        items[0].add_label(Label::new("old".into(), Vec2::ZERO));
        let mut selected = HashSet::new();
        let mut stack = UndoStack::default();

        items[0].labels_mut().unwrap()[0].text = "new".into();
        stack.push(Command::EditLabel {
            item_idx: 0,
            label_idx: 0,
            old_text: "old".into(),
            new_text: "new".into(),
        });

        stack.undo(&mut items, &mut selected);
        assert_eq!(items[0].labels()[0].text, "old");

        stack.redo(&mut items, &mut selected);
        assert_eq!(items[0].labels()[0].text, "new");
    }
}
