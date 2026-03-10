use std::collections::HashSet;

use egui::{Color32, CursorIcon, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};

use crate::items::BoardItem;

use super::interaction::Corner;

pub const SELECTION_COLOR: Color32 = Color32::from_rgb(100, 160, 255);
pub const HANDLE_SIZE: f32 = 8.0;
pub const ROT_HANDLE_OFFSET: f32 = 25.0;

pub enum HandleHit {
    Resize(Corner),
    Rotate,
}

pub fn draw_grid(ui: &mut Ui, visible_rect: Rect, grid_size: f32) {
    if grid_size < 5.0 {
        return;
    }

    let grid_color = Color32::from_rgba_premultiplied(255, 255, 255, 15);
    let stroke = Stroke::new(0.5, grid_color);

    let mut shapes = Vec::new();

    // Vertical lines
    let start_x = (visible_rect.min.x / grid_size).floor() * grid_size;
    let mut x = start_x;
    while x <= visible_rect.max.x {
        shapes.push(egui::Shape::line_segment(
            [Pos2::new(x, visible_rect.min.y), Pos2::new(x, visible_rect.max.y)],
            stroke,
        ));
        x += grid_size;
    }

    // Horizontal lines
    let start_y = (visible_rect.min.y / grid_size).floor() * grid_size;
    let mut y = start_y;
    while y <= visible_rect.max.y {
        shapes.push(egui::Shape::line_segment(
            [Pos2::new(visible_rect.min.x, y), Pos2::new(visible_rect.max.x, y)],
            stroke,
        ));
        y += grid_size;
    }

    ui.painter().extend(shapes);
}

pub fn draw_selection_handles(
    ui: &mut Ui,
    items: &[BoardItem],
    selected: &HashSet<usize>,
    pointer_pos: Option<Pos2>,
) -> Option<HandleHit> {
    let group_rect = super::interaction::selected_bounding_rect(items, selected)?;

    // Group border
    ui.painter().rect_stroke(
        group_rect,
        0.0,
        Stroke::new(1.5, SELECTION_COLOR),
        StrokeKind::Outside,
    );

    let mut hit: Option<HandleHit> = None;

    // Corner resize handles
    for corner in Corner::ALL {
        let pos = corner.pos_in_rect(group_rect);
        let handle_rect = Rect::from_center_size(pos, Vec2::splat(HANDLE_SIZE));
        ui.painter().rect_filled(handle_rect, 1.0, SELECTION_COLOR);

        if let Some(p) = pointer_pos {
            if handle_rect.expand(4.0).contains(p) {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeNwSe);
                if hit.is_none() {
                    hit = Some(HandleHit::Resize(corner));
                }
            }
        }
    }

    // Rotation handle above top center
    let rot_pos = Pos2::new(group_rect.center().x, group_rect.top() - ROT_HANDLE_OFFSET);
    let rot_rect = Rect::from_center_size(rot_pos, Vec2::splat(HANDLE_SIZE));

    ui.painter().line_segment(
        [Pos2::new(group_rect.center().x, group_rect.top()), rot_pos],
        Stroke::new(1.0, SELECTION_COLOR),
    );
    ui.painter()
        .circle_filled(rot_pos, HANDLE_SIZE / 2.0, SELECTION_COLOR);

    if let Some(p) = pointer_pos {
        if rot_rect.expand(4.0).contains(p) {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            hit = Some(HandleHit::Rotate);
        }
    }

    hit
}

pub fn draw_item(ui: &mut Ui, item: &BoardItem, is_selected: bool) {
    let rotation = item.transform().rotation;

    match item {
        BoardItem::Image(img) => {
            let rect = item.bounding_rect();

            // Draw placeholder if texture not yet decoded
            let Some(texture) = &img.texture else {
                ui.painter().rect_filled(rect, 2.0, Color32::from_gray(60));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Loading...",
                    egui::FontId::proportional(12.0),
                    Color32::from_gray(140),
                );
                if is_selected {
                    ui.painter().rect_stroke(
                        rect,
                        0.0,
                        Stroke::new(1.0, SELECTION_COLOR.gamma_multiply(0.5)),
                        StrokeKind::Outside,
                    );
                }
                return;
            };

            // UV coords: map crop_rect (in pixel space) to 0..1 UV range
            let (mut u_min, mut u_max, mut v_min, mut v_max) = if let Some(cr) = img.crop_rect {
                (
                    cr.min.x / img.original_size.x,
                    cr.max.x / img.original_size.x,
                    cr.min.y / img.original_size.y,
                    cr.max.y / img.original_size.y,
                )
            } else {
                (0.0, 1.0, 0.0, 1.0)
            };

            if img.flip_h {
                std::mem::swap(&mut u_min, &mut u_max);
            }
            if img.flip_v {
                std::mem::swap(&mut v_min, &mut v_max);
            }

            let tint = Color32::from_rgba_unmultiplied(255, 255, 255, (img.opacity * 255.0) as u8);

            if rotation.abs() > 0.001 {
                let center = rect.center();
                let cos = rotation.cos();
                let sin = rotation.sin();

                let rotate_point = |p: Pos2| -> Pos2 {
                    let dx = p.x - center.x;
                    let dy = p.y - center.y;
                    Pos2::new(
                        center.x + dx * cos - dy * sin,
                        center.y + dx * sin + dy * cos,
                    )
                };

                let tl = rotate_point(rect.left_top());
                let tr = rotate_point(rect.right_top());
                let bl = rotate_point(rect.left_bottom());
                let br = rotate_point(rect.right_bottom());

                let mut mesh = egui::Mesh::with_texture(texture.id());
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: tl,
                    uv: egui::pos2(u_min, v_min),
                    color: tint,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: tr,
                    uv: egui::pos2(u_max, v_min),
                    color: tint,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: br,
                    uv: egui::pos2(u_max, v_max),
                    color: tint,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: bl,
                    uv: egui::pos2(u_min, v_max),
                    color: tint,
                });
                mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                ui.painter().add(egui::Shape::mesh(mesh));
            } else {
                let uv = Rect::from_min_max(
                    egui::pos2(u_min, v_min),
                    egui::pos2(u_max, v_max),
                );
                let mut mesh = egui::Mesh::with_texture(texture.id());
                mesh.add_rect_with_uv(rect, uv, tint);
                ui.painter().add(egui::Shape::mesh(mesh));
            }

            if is_selected {
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, SELECTION_COLOR.gamma_multiply(0.5)),
                    StrokeKind::Outside,
                );
            }

            // Draw labels
            let image_pos = img.transform.position;
            for label in &img.labels {
                let label_pos = (image_pos + label.offset).to_pos2();
                let label_rect = label.bounding_rect(image_pos);
                ui.painter().rect_filled(
                    label_rect,
                    2.0,
                    Color32::from_rgba_premultiplied(0, 0, 0, 180),
                );
                ui.painter().text(
                    label_pos + Vec2::new(4.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    &label.text,
                    egui::FontId::proportional(label.font_size),
                    label.color,
                );
            }
        }
        BoardItem::Text(txt) => {
            let pos = txt.transform.position.to_pos2();
            ui.painter().text(
                pos,
                egui::Align2::LEFT_TOP,
                &txt.content,
                egui::FontId::proportional(txt.font_size),
                txt.color,
            );

            if is_selected {
                let rect = item.bounding_rect();
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, SELECTION_COLOR.gamma_multiply(0.5)),
                    StrokeKind::Outside,
                );
            }
        }
    }
}
