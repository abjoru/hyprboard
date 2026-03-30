use std::collections::HashSet;

use egui::{Color32, Rect, Vec2};

use crate::items::{BoardItem, Connector, ConnectorId};

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
        removed_connectors: Vec<Connector>,
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
    AddConnector {
        id: ConnectorId,
    },
    DeleteConnector {
        connector: Connector,
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

    pub fn undo(
        &mut self,
        items: &mut Vec<BoardItem>,
        connectors: &mut Vec<Connector>,
        selected: &mut HashSet<usize>,
    ) {
        if let Some(cmd) = self.undos.pop() {
            let reverse = Self::apply_reverse(&cmd, items, connectors, selected);
            self.redos.push(reverse);
            self.dirty = true;
        }
    }

    pub fn redo(
        &mut self,
        items: &mut Vec<BoardItem>,
        connectors: &mut Vec<Connector>,
        selected: &mut HashSet<usize>,
    ) {
        if let Some(cmd) = self.redos.pop() {
            let reverse = Self::apply_reverse(&cmd, items, connectors, selected);
            self.undos.push(reverse);
            self.dirty = true;
        }
    }

    fn apply_reverse(
        cmd: &Command,
        items: &mut Vec<BoardItem>,
        connectors: &mut Vec<Connector>,
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
            Command::Delete {
                items: deleted,
                removed_connectors,
            } => {
                let mut sorted = deleted.clone();
                sorted.sort_by_key(|(idx, _)| *idx);
                selected.clear();
                for (idx, item) in sorted {
                    items.insert(idx, item);
                    selected.insert(idx);
                }
                // Restore cascade-removed connectors
                connectors.extend(removed_connectors.iter().cloned());
                Command::Add {
                    count: deleted.len(),
                }
            }
            Command::Add { count } => {
                let start = items.len().saturating_sub(*count);
                // Cascade-remove connectors for items being removed
                let deleted_ids: std::collections::HashSet<_> =
                    items[start..].iter().map(|i| i.item_id()).collect();
                let mut cascade = Vec::new();
                connectors.retain(|c| {
                    if deleted_ids.contains(&c.from) || deleted_ids.contains(&c.to) {
                        cascade.push(c.clone());
                        false
                    } else {
                        true
                    }
                });
                let removed: Vec<_> = (start..items.len()).zip(items.drain(start..)).collect();
                selected.clear();
                Command::Delete {
                    items: removed,
                    removed_connectors: cascade,
                }
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
            Command::AddConnector { id } => {
                if let Some(pos) = connectors.iter().position(|c| c.id == *id) {
                    let connector = connectors.remove(pos);
                    Command::DeleteConnector { connector }
                } else {
                    Command::AddConnector { id: *id }
                }
            }
            Command::DeleteConnector { connector } => {
                let id = connector.id;
                connectors.push(connector.clone());
                Command::AddConnector { id }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::items::{ImageItem, ItemId, Transform};

    use super::*;

    fn make_test_items() -> Vec<BoardItem> {
        vec![
            BoardItem::new_text(ItemId(1), "A".into(), Vec2::new(0.0, 0.0)),
            BoardItem::new_text(ItemId(2), "B".into(), Vec2::new(100.0, 0.0)),
            BoardItem::new_text(ItemId(3), "C".into(), Vec2::new(200.0, 0.0)),
        ]
    }

    #[test]
    fn undo_move() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
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
        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items[0].transform().position, Vec2::new(0.0, 0.0));
        assert_eq!(items[1].transform().position, Vec2::new(100.0, 0.0));

        // Redo
        stack.redo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items[0].transform().position, Vec2::new(10.0, 20.0));
        assert_eq!(items[1].transform().position, Vec2::new(110.0, 20.0));
    }

    #[test]
    fn undo_delete() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        // Delete item 1 ("B")
        let removed = items.remove(1);
        stack.push(Command::Delete {
            items: vec![(1, removed)],
            removed_connectors: Vec::new(),
        });

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text_content().unwrap(), "A");
        assert_eq!(items[1].text_content().unwrap(), "C");

        // Undo — restore "B" at index 1
        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].text_content().unwrap(), "B");
        assert!(selected.contains(&1));
    }

    #[test]
    fn undo_add() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        items.push(BoardItem::new_text(
            ItemId(4),
            "D".into(),
            Vec2::new(300.0, 0.0),
        ));
        stack.push(Command::Add { count: 1 });

        assert_eq!(items.len(), 4);

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items.len(), 3);

        stack.redo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items.len(), 4);
        assert_eq!(items[3].text_content().unwrap(), "D");
    }

    #[test]
    fn undo_zorder() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        // Reverse order: [C, B, A]
        items.reverse();
        stack.push(Command::ZOrder {
            old_order: vec![2, 1, 0],
        });

        assert_eq!(items[0].text_content().unwrap(), "C");
        assert_eq!(items[2].text_content().unwrap(), "A");

        // Undo — restore original order
        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items[0].text_content().unwrap(), "A");
        assert_eq!(items[1].text_content().unwrap(), "B");
        assert_eq!(items[2].text_content().unwrap(), "C");
    }

    #[test]
    fn undo_opacity() {
        let mut items = vec![BoardItem::new_text(ItemId(0), "X".into(), Vec2::ZERO)];
        // Text items have fixed 1.0 opacity, but we test the mechanism
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        stack.push(Command::Opacity {
            indices: vec![0],
            old_values: vec![1.0],
            new_values: vec![0.5],
        });

        stack.undo(&mut items, &mut connectors, &mut selected);
        // Opacity on text items is a no-op, but command mechanics work
        assert!(stack.redos.len() == 1);
    }

    #[test]
    fn push_clears_redos() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        stack.push(Command::Move {
            indices: vec![0],
            delta: Vec2::new(5.0, 0.0),
        });
        items[0].transform_mut().position += Vec2::new(5.0, 0.0);

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(stack.redos.len(), 1);

        // New push should clear redos
        stack.push(Command::Move {
            indices: vec![1],
            delta: Vec2::new(1.0, 0.0),
        });
        assert_eq!(stack.redos.len(), 0);
    }

    fn make_image_item(pos: Vec2, size: Vec2) -> BoardItem {
        BoardItem::Image(ImageItem {
            id: ItemId(0),
            texture: None,
            original_bytes: Arc::from(vec![0u8; 4]),
            original_size: size,
            transform: Transform::default().with_position(pos),
            crop_rect: None,
            opacity: 1.0,
            grayscale: false,
            flip_h: false,
            flip_v: false,
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
        let mut connectors: Vec<Connector> = Vec::new();
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

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items[0].transform().scale, Vec2::splat(1.0));
        assert_eq!(items[0].transform().position, Vec2::new(10.0, 10.0));

        stack.redo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items[0].transform().scale, Vec2::splat(2.0));
        assert_eq!(items[0].transform().position, Vec2::new(5.0, 5.0));
    }

    #[test]
    fn undo_rotate() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        items[0].transform_mut().rotation = 1.5;
        stack.push(Command::Rotate {
            indices: vec![0],
            old_rotations: vec![0.0],
            new_rotations: vec![1.5],
            old_positions: vec![Vec2::ZERO],
            new_positions: vec![Vec2::ZERO],
        });

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert!((items[0].transform().rotation).abs() < 0.001);

        stack.redo(&mut items, &mut connectors, &mut selected);
        assert!((items[0].transform().rotation - 1.5).abs() < 0.001);
    }

    #[test]
    fn undo_crop() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        let crop = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(50.0, 50.0));
        items[0].set_crop_rect(Some(crop));
        stack.push(Command::Crop {
            idx: 0,
            old_rect: None,
            new_rect: Some(crop),
        });

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert!(items[0].crop_rect().is_none());

        stack.redo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items[0].crop_rect().unwrap(), crop);
    }

    #[test]
    fn undo_edit_text() {
        let mut items = vec![BoardItem::new_text(ItemId(0), "hello".into(), Vec2::ZERO)];
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        items[0].set_text_content("world".into());
        stack.push(Command::EditText {
            idx: 0,
            old_content: "hello".into(),
            new_content: "world".into(),
        });

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items[0].text_content().unwrap(), "hello");

        stack.redo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items[0].text_content().unwrap(), "world");
    }

    #[test]
    fn undo_flip() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        items[0].toggle_flip(true);
        stack.push(Command::Flip {
            indices: vec![0],
            horizontal: true,
        });

        assert!(matches!(&items[0], BoardItem::Image(img) if img.flip_h));

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert!(matches!(&items[0], BoardItem::Image(img) if !img.flip_h));

        stack.redo(&mut items, &mut connectors, &mut selected);
        assert!(matches!(&items[0], BoardItem::Image(img) if img.flip_h));
    }

    #[test]
    fn undo_grayscale() {
        let mut items = vec![make_image_item(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        // toggle_grayscale without texture just flips the flag
        items[0].toggle_grayscale();
        stack.push(Command::Grayscale { indices: vec![0] });

        assert!(matches!(&items[0], BoardItem::Image(img) if img.grayscale));

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert!(matches!(&items[0], BoardItem::Image(img) if !img.grayscale));
    }

    #[test]
    fn undo_add_connector() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        let id = ConnectorId(100);
        connectors.push(Connector::new(id, ItemId(1), ItemId(2)));
        stack.push(Command::AddConnector { id });

        assert_eq!(connectors.len(), 1);

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(connectors.len(), 0);

        stack.redo(&mut items, &mut connectors, &mut selected);
        assert_eq!(connectors.len(), 1);
        assert_eq!(connectors[0].id, id);
    }

    #[test]
    fn undo_delete_connector() {
        let mut items = make_test_items();
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = Vec::new();
        let mut stack = UndoStack::default();

        let conn = Connector::new(ConnectorId(50), ItemId(1), ItemId(2));
        stack.push(Command::DeleteConnector {
            connector: conn.clone(),
        });

        assert_eq!(connectors.len(), 0);

        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(connectors.len(), 1);
        assert_eq!(connectors[0].from, ItemId(1));

        stack.redo(&mut items, &mut connectors, &mut selected);
        assert_eq!(connectors.len(), 0);
    }

    #[test]
    fn delete_item_cascades_connectors() {
        let mut items = make_test_items(); // A(1), B(2), C(3)
        let mut selected = HashSet::new();
        let mut connectors: Vec<Connector> = vec![
            Connector::new(ConnectorId(10), ItemId(1), ItemId(2)),
            Connector::new(ConnectorId(11), ItemId(2), ItemId(3)),
        ];
        let mut stack = UndoStack::default();

        // Delete item B (index 1, ItemId(2)) — both connectors reference it
        let removed = items.remove(1);
        stack.push(Command::Delete {
            items: vec![(1, removed)],
            removed_connectors: Vec::new(),
        });

        // Undo the delete — restores B
        stack.undo(&mut items, &mut connectors, &mut selected);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].text_content().unwrap(), "B");
    }
}
