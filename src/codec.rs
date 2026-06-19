//! Storage codec: converts between domain types and storage-shaped primitives.
//!
//! Persistence (SQLite) and other storage backends share these conversions so
//! the encoding lives in one canonical place, free of backend-specific types.

use std::sync::Arc;

use egui::{Color32, Rect, Vec2};

use crate::items::{BoardItem, ImageItem, ItemId, TextItem, Transform, image_dimensions};

/// Storage-shaped mirror of a [`BoardItem`]: primitives only (floats, `u32`
/// colors, a reference-counted byte blob, optional tuples). Derived state that
/// is never persisted — the GPU texture handle and cached text size — has no
/// home here; conversion back to a board item supplies the defaults.
#[derive(Clone, Debug)]
pub enum ItemRecord {
    Image(ImageRecord),
    Text(TextRecord),
}

#[derive(Clone, Debug)]
pub struct ImageRecord {
    pub id: u64,
    pub pos: (f32, f32),
    pub scale: (f32, f32),
    pub rotation: f32,
    /// Encoded source bytes, shared by handle — never deep-copied on convert.
    pub bytes: Arc<[u8]>,
    /// Stored pixel dimensions; `None` for legacy rows where the column is
    /// absent, in which case [`ItemRecord::into_item`] probes the bytes.
    pub dimensions: Option<(f32, f32)>,
    /// Crop as `(x, y, w, h)` in source pixels.
    pub crop: Option<(f32, f32, f32, f32)>,
    pub opacity: f32,
    pub grayscale: bool,
    pub flip_h: bool,
    pub flip_v: bool,
    pub border_color: u32,
}

#[derive(Clone, Debug)]
pub struct TextRecord {
    pub id: u64,
    pub pos: (f32, f32),
    pub scale: (f32, f32),
    pub rotation: f32,
    pub content: String,
    pub font_size: f32,
    pub color: u32,
    pub bg_color: u32,
    pub border_color: u32,
}

impl ItemRecord {
    /// Project a live board item onto its storage mirror, dropping derived
    /// state (texture handle, cached text size). Image bytes are carried by
    /// reference-counted handle — never deep-copied.
    pub fn from_item(item: &BoardItem) -> Self {
        match item {
            BoardItem::Image(img) => ItemRecord::Image(ImageRecord {
                id: img.id.0,
                pos: (img.transform.position.x, img.transform.position.y),
                scale: (img.transform.scale.x, img.transform.scale.y),
                rotation: img.transform.rotation,
                bytes: img.original_bytes.clone(),
                dimensions: Some((img.original_size.x, img.original_size.y)),
                crop: img
                    .crop_rect
                    .map(|r| (r.min.x, r.min.y, r.width(), r.height())),
                opacity: img.opacity,
                grayscale: img.grayscale,
                flip_h: img.flip_h,
                flip_v: img.flip_v,
                border_color: color_to_u32(img.border_color),
            }),
            BoardItem::Text(txt) => ItemRecord::Text(TextRecord {
                id: txt.id.0,
                pos: (txt.transform.position.x, txt.transform.position.y),
                scale: (txt.transform.scale.x, txt.transform.scale.y),
                rotation: txt.transform.rotation,
                content: txt.content.clone(),
                font_size: txt.font_size,
                color: color_to_u32(txt.color),
                bg_color: color_to_u32(txt.bg_color),
                border_color: color_to_u32(txt.border_color),
            }),
        }
    }

    /// Rebuild a board item from its storage mirror. Sets the texture handle to
    /// none and cached text size to zero, assembles the transform, decodes
    /// colors, rebuilds the crop rect, and — for images with no stored
    /// dimensions — probes the bytes (falling back to 100×100 on decode error).
    pub fn into_item(self) -> BoardItem {
        match self {
            ItemRecord::Image(r) => {
                let original_size = match r.dimensions {
                    Some((w, h)) => Vec2::new(w, h),
                    None => image_dimensions(&r.bytes).unwrap_or(Vec2::new(100.0, 100.0)),
                };
                let crop_rect = r
                    .crop
                    .map(|(x, y, w, h)| Rect::from_min_size(egui::pos2(x, y), Vec2::new(w, h)));
                BoardItem::Image(ImageItem {
                    id: ItemId(r.id),
                    texture: None,
                    original_bytes: r.bytes,
                    original_size,
                    transform: record_transform(r.pos, r.scale, r.rotation),
                    crop_rect,
                    opacity: r.opacity,
                    grayscale: r.grayscale,
                    flip_h: r.flip_h,
                    flip_v: r.flip_v,
                    border_color: u32_to_color(r.border_color),
                })
            }
            ItemRecord::Text(r) => BoardItem::Text(TextItem {
                id: ItemId(r.id),
                content: r.content,
                font_size: r.font_size,
                color: u32_to_color(r.color),
                bg_color: u32_to_color(r.bg_color),
                border_color: u32_to_color(r.border_color),
                transform: record_transform(r.pos, r.scale, r.rotation),
                cached_size: Vec2::ZERO,
            }),
        }
    }
}

fn record_transform(pos: (f32, f32), scale: (f32, f32), rotation: f32) -> Transform {
    Transform {
        position: Vec2::new(pos.0, pos.1),
        rotation,
        scale: Vec2::new(scale.0, scale.1),
    }
}

/// Encode a color as a big-endian `0xRRGGBBAA` `u32`.
pub fn color_to_u32(c: Color32) -> u32 {
    u32::from_be_bytes([c.r(), c.g(), c.b(), c.a()])
}

/// Decode a big-endian `0xRRGGBBAA` `u32` into a non-premultiplied color.
pub fn u32_to_color(v: u32) -> Color32 {
    let [r, g, b, a] = v.to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_u32_round_trip() {
        // Color32 stores premultiplied RGBA; opaque colors carry their channels
        // verbatim, so they round-trip exactly — alpha included.
        let opaque = [
            Color32::from_rgba_unmultiplied(0, 0, 0, 255),
            Color32::from_rgba_unmultiplied(255, 255, 255, 255),
            Color32::from_rgba_unmultiplied(12, 34, 56, 255),
            Color32::from_rgba_unmultiplied(255, 0, 128, 255),
        ];
        for c in opaque {
            assert_eq!(u32_to_color(color_to_u32(c)), c);
        }

        // Fully transparent collapses to all-zero and round-trips.
        let clear = Color32::from_rgba_unmultiplied(0, 0, 0, 0);
        assert_eq!(u32_to_color(color_to_u32(clear)), clear);
    }

    #[test]
    fn color_u32_layout_is_big_endian_rgba() {
        // 0xRRGGBBAA byte order, alpha in the low byte.
        let c = Color32::from_rgba_unmultiplied(0x11, 0x22, 0x33, 0xFF);
        assert_eq!(color_to_u32(c), 0x1122_33FF);
        assert_eq!(u32_to_color(0x1122_33FF), c);
    }

    fn make_test_png(w: u32, h: u32) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([255, 0, 0, 255]);
        }
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn image_record_round_trip() {
        let png = make_test_png(10, 10);
        let original = BoardItem::Image(ImageItem {
            id: ItemId(7),
            texture: None,
            original_bytes: Arc::from(png.as_slice()),
            original_size: Vec2::new(10.0, 10.0),
            transform: Transform {
                position: Vec2::new(42.0, 84.0),
                rotation: 1.5,
                scale: Vec2::new(2.0, 3.0),
            },
            crop_rect: Some(Rect::from_min_size(
                egui::pos2(1.0, 2.0),
                Vec2::new(5.0, 6.0),
            )),
            opacity: 0.7,
            grayscale: true,
            flip_h: true,
            flip_v: false,
            border_color: Color32::from_rgb(10, 20, 30),
        });

        let back = ItemRecord::from_item(&original).into_item();
        let BoardItem::Image(img) = back else {
            panic!("expected Image");
        };
        assert_eq!(img.id, ItemId(7));
        assert_eq!(img.transform.position, Vec2::new(42.0, 84.0));
        assert_eq!(img.transform.rotation, 1.5);
        assert_eq!(img.transform.scale, Vec2::new(2.0, 3.0));
        let cr = img.crop_rect.expect("crop preserved");
        assert_eq!(cr.min, egui::pos2(1.0, 2.0));
        assert_eq!(cr.size(), Vec2::new(5.0, 6.0));
        assert_eq!(img.opacity, 0.7);
        assert!(img.grayscale);
        assert!(img.flip_h);
        assert!(!img.flip_v);
        assert_eq!(img.border_color, Color32::from_rgb(10, 20, 30));
        assert_eq!(&img.original_bytes[..], &png[..]);
        // Derived state defaults on rebuild.
        assert!(img.texture.is_none());
    }

    #[test]
    fn text_record_round_trip() {
        let original = BoardItem::Text(TextItem {
            id: ItemId(9),
            content: "hello world".into(),
            font_size: 24.0,
            color: Color32::from_rgb(0, 255, 128),
            bg_color: Color32::from_rgb(20, 20, 20),
            border_color: Color32::from_rgb(200, 100, 50),
            transform: Transform {
                position: Vec2::new(100.0, 200.0),
                rotation: 0.25,
                scale: Vec2::new(1.5, 1.5),
            },
            cached_size: Vec2::new(123.0, 45.0),
        });

        let back = ItemRecord::from_item(&original).into_item();
        let BoardItem::Text(txt) = back else {
            panic!("expected Text");
        };
        assert_eq!(txt.id, ItemId(9));
        assert_eq!(txt.content, "hello world");
        assert_eq!(txt.font_size, 24.0);
        assert_eq!(txt.color, Color32::from_rgb(0, 255, 128));
        assert_eq!(txt.bg_color, Color32::from_rgb(20, 20, 20));
        assert_eq!(txt.border_color, Color32::from_rgb(200, 100, 50));
        assert_eq!(txt.transform.position, Vec2::new(100.0, 200.0));
        assert_eq!(txt.transform.rotation, 0.25);
        assert_eq!(txt.transform.scale, Vec2::new(1.5, 1.5));
        // Cached size is derived, reset on rebuild.
        assert_eq!(txt.cached_size, Vec2::ZERO);
    }

    #[test]
    fn image_record_bytes_share_handle() {
        let png = make_test_png(2, 2);
        let bytes: Arc<[u8]> = Arc::from(png.as_slice());
        let item = BoardItem::Image(ImageItem {
            id: ItemId(1),
            texture: None,
            original_bytes: bytes.clone(),
            original_size: Vec2::new(2.0, 2.0),
            transform: Transform::default(),
            crop_rect: None,
            opacity: 1.0,
            grayscale: false,
            flip_h: false,
            flip_v: false,
            border_color: Color32::TRANSPARENT,
        });
        let strong_before = Arc::strong_count(&bytes);
        let record = ItemRecord::from_item(&item);
        // Record holds the same allocation, not a copy.
        assert!(Arc::strong_count(&bytes) > strong_before);
        let BoardItem::Image(img) = record.into_item() else {
            panic!("expected Image");
        };
        assert!(Arc::ptr_eq(&bytes, &img.original_bytes));
    }

    #[test]
    fn dimensions_present_used_verbatim() {
        // 4×4 PNG but record claims 200×100 — verbatim wins, no probe.
        let png = make_test_png(4, 4);
        let record = ItemRecord::Image(ImageRecord {
            id: 1,
            pos: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation: 0.0,
            bytes: Arc::from(png.as_slice()),
            dimensions: Some((200.0, 100.0)),
            crop: None,
            opacity: 1.0,
            grayscale: false,
            flip_h: false,
            flip_v: false,
            border_color: 0,
        });
        let BoardItem::Image(img) = record.into_item() else {
            panic!("expected Image");
        };
        assert_eq!(img.original_size, Vec2::new(200.0, 100.0));
    }

    #[test]
    fn dimensions_absent_probes_bytes() {
        let png = make_test_png(12, 7);
        let record = ItemRecord::Image(ImageRecord {
            id: 1,
            pos: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation: 0.0,
            bytes: Arc::from(png.as_slice()),
            dimensions: None,
            crop: None,
            opacity: 1.0,
            grayscale: false,
            flip_h: false,
            flip_v: false,
            border_color: 0,
        });
        let BoardItem::Image(img) = record.into_item() else {
            panic!("expected Image");
        };
        assert_eq!(img.original_size, Vec2::new(12.0, 7.0));
    }

    #[test]
    fn dimensions_absent_undecodable_falls_back() {
        let record = ItemRecord::Image(ImageRecord {
            id: 1,
            pos: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation: 0.0,
            bytes: Arc::from([0u8, 1, 2, 3].as_slice()),
            dimensions: None,
            crop: None,
            opacity: 1.0,
            grayscale: false,
            flip_h: false,
            flip_v: false,
            border_color: 0,
        });
        let BoardItem::Image(img) = record.into_item() else {
            panic!("expected Image");
        };
        assert_eq!(img.original_size, Vec2::new(100.0, 100.0));
    }
}
