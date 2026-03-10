use std::io::Write;
use std::process::{Command, Stdio};

use image::{imageops, RgbaImage};

use crate::items::BoardItem;

fn wl_copy(mime_type: &str, data: &[u8]) -> Result<(), String> {
    let mut child = Command::new("wl-copy")
        .args(["--type", mime_type])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("wl-copy spawn: {e}"))?;

    child
        .stdin
        .as_mut()
        .ok_or("wl-copy: no stdin")?
        .write_all(data)
        .map_err(|e| format!("wl-copy write: {e}"))?;

    let status = child.wait().map_err(|e| format!("wl-copy wait: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wl-copy exited {status}"))
    }
}

pub fn copy_image_to_clipboard(png_bytes: &[u8]) -> Result<(), String> {
    wl_copy("image/png", png_bytes)
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    wl_copy("text/plain", text.as_bytes())
}

/// Encode original_bytes as PNG if not already PNG.
pub fn ensure_png(original_bytes: &[u8]) -> Result<Vec<u8>, String> {
    // Check PNG magic bytes
    if original_bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Ok(original_bytes.to_vec());
    }
    // Re-encode as PNG
    let img = image::load_from_memory(original_bytes)
        .map_err(|e| format!("decode: {e}"))?;
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("encode png: {e}"))?;
    Ok(buf)
}

/// Composite multiple items into a single PNG (2x resolution).
/// Items should be in z-order (bottom to top).
pub fn render_collage(items: &[&BoardItem]) -> Result<Vec<u8>, String> {
    if items.is_empty() {
        return Err("no items".into());
    }

    // Compute union bounding rect
    let mut union_rect = items[0].bounding_rect();
    for item in &items[1..] {
        union_rect = union_rect.union(item.bounding_rect());
    }

    render_to_png(items, union_rect)
}

/// Render items within a specified region rect to PNG (2x resolution).
pub fn render_region(items: &[&BoardItem], region: egui::Rect) -> Result<Vec<u8>, String> {
    if items.is_empty() {
        return Err("no items in region".into());
    }
    render_to_png(items, region)
}

fn render_to_png(items: &[&BoardItem], rect: egui::Rect) -> Result<Vec<u8>, String> {
    let scale_factor = 2.0_f32;
    let canvas_w = (rect.width() * scale_factor).ceil() as u32;
    let canvas_h = (rect.height() * scale_factor).ceil() as u32;

    if canvas_w == 0 || canvas_h == 0 {
        return Err("zero-size canvas".into());
    }

    let mut canvas = RgbaImage::new(canvas_w, canvas_h);

    for item in items {
        let rendered = render_item(item, scale_factor)?;
        let item_rect = item.bounding_rect();
        let x = ((item_rect.min.x - rect.min.x) * scale_factor).round() as i64;
        let y = ((item_rect.min.y - rect.min.y) * scale_factor).round() as i64;

        imageops::overlay(&mut canvas, &rendered, x, y);
    }

    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    canvas
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("encode png: {e}"))?;
    Ok(buf)
}

/// Render a single BoardItem to an RgbaImage with all transforms applied.
fn render_item(item: &BoardItem, scale_factor: f32) -> Result<RgbaImage, String> {
    match item {
        BoardItem::Image(img) => apply_transforms(
            &img.original_bytes,
            img.original_size,
            &img.transform,
            img.crop_rect,
            img.opacity,
            img.grayscale,
            img.flip_h,
            img.flip_v,
            scale_factor,
        ),
        BoardItem::Text(_) => {
            // Text items: return empty 1x1 transparent (text not rendered in collage)
            Ok(RgbaImage::new(1, 1))
        }
    }
}

/// Core image transform pipeline — decoupled from BoardItem for testability.
pub fn apply_transforms(
    image_bytes: &[u8],
    original_size: egui::Vec2,
    transform: &crate::items::Transform,
    crop_rect: Option<egui::Rect>,
    opacity: f32,
    grayscale: bool,
    flip_h: bool,
    flip_v: bool,
    scale_factor: f32,
) -> Result<RgbaImage, String> {
    let img = image::load_from_memory(image_bytes)
        .map_err(|e| format!("decode: {e}"))?;

    let mut rgba = if grayscale {
        img.grayscale().to_rgba8()
    } else {
        img.to_rgba8()
    };

    // Apply crop
    if let Some(cr) = crop_rect {
        let cx = cr.min.x.max(0.0) as u32;
        let cy = cr.min.y.max(0.0) as u32;
        let cw = (cr.width() as u32).min(rgba.width().saturating_sub(cx));
        let ch = (cr.height() as u32).min(rgba.height().saturating_sub(cy));
        if cw > 0 && ch > 0 {
            rgba = imageops::crop_imm(&rgba, cx, cy, cw, ch).to_image();
        }
    }

    // Apply flip
    if flip_h {
        imageops::flip_horizontal_in_place(&mut rgba);
    }
    if flip_v {
        imageops::flip_vertical_in_place(&mut rgba);
    }

    // Scale to display size * scale_factor
    let base_size = crop_rect
        .map(|r| r.size())
        .unwrap_or(original_size);
    let display_w = (base_size.x * transform.scale.x * scale_factor).round() as u32;
    let display_h = (base_size.y * transform.scale.y * scale_factor).round() as u32;

    if display_w > 0 && display_h > 0 {
        rgba = imageops::resize(&rgba, display_w, display_h, imageops::FilterType::Lanczos3);
    }

    // Apply rotation
    if transform.rotation.abs() > 0.001 {
        rgba = imageproc::geometric_transformations::rotate_about_center(
            &rgba,
            transform.rotation,
            imageproc::geometric_transformations::Interpolation::Bilinear,
            image::Rgba([0, 0, 0, 0]),
        );
    }

    // Apply opacity
    if opacity < 1.0 {
        let alpha_mult = (opacity * 255.0) as u8;
        for pixel in rgba.pixels_mut() {
            pixel.0[3] = ((pixel.0[3] as u16 * alpha_mult as u16) / 255) as u8;
        }
    }

    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::Transform;
    use egui::Vec2;

    /// Create a minimal 4x4 red PNG in memory.
    fn make_test_png(w: u32, h: u32) -> Vec<u8> {
        let mut img = RgbaImage::new(w, h);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([255, 0, 0, 255]);
        }
        let mut buf = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
        buf
    }

    /// Create a minimal JPEG in memory.
    fn make_test_jpeg(w: u32, h: u32) -> Vec<u8> {
        let mut img = RgbaImage::new(w, h);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([0, 0, 255, 255]);
        }
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        let mut buf = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buf);
        rgb.write_to(&mut cursor, image::ImageFormat::Jpeg).unwrap();
        buf
    }

    // --- ensure_png ---

    #[test]
    fn ensure_png_passthrough_for_png() {
        let png = make_test_png(4, 4);
        let result = ensure_png(&png).unwrap();
        assert_eq!(result, png);
    }

    #[test]
    fn ensure_png_reencodes_jpeg() {
        let jpeg = make_test_jpeg(4, 4);
        assert!(!jpeg.starts_with(&[0x89, b'P', b'N', b'G']));
        let result = ensure_png(&jpeg).unwrap();
        assert!(result.starts_with(&[0x89, b'P', b'N', b'G']));
        let img = image::load_from_memory(&result).unwrap();
        assert_eq!(img.width(), 4);
        assert_eq!(img.height(), 4);
    }

    #[test]
    fn ensure_png_errors_on_garbage() {
        let result = ensure_png(b"not an image");
        assert!(result.is_err());
    }

    // --- apply_transforms: identity ---

    #[test]
    fn transforms_identity() {
        let png = make_test_png(10, 10);
        let transform = Transform::default();
        let result = apply_transforms(
            &png,
            Vec2::new(10.0, 10.0),
            &transform,
            None, 1.0, false, false, false,
            1.0,
        ).unwrap();
        assert_eq!(result.width(), 10);
        assert_eq!(result.height(), 10);
        for pixel in result.pixels() {
            assert_eq!(pixel.0, [255, 0, 0, 255]);
        }
    }

    // --- apply_transforms: scale ---

    #[test]
    fn transforms_scale_2x() {
        let png = make_test_png(10, 10);
        let mut transform = Transform::default();
        transform.scale = Vec2::splat(2.0);
        let result = apply_transforms(
            &png,
            Vec2::new(10.0, 10.0),
            &transform,
            None, 1.0, false, false, false,
            1.0,
        ).unwrap();
        assert_eq!(result.width(), 20);
        assert_eq!(result.height(), 20);
    }

    #[test]
    fn transforms_scale_factor() {
        let png = make_test_png(10, 10);
        let transform = Transform::default();
        let result = apply_transforms(
            &png,
            Vec2::new(10.0, 10.0),
            &transform,
            None, 1.0, false, false, false,
            2.0,
        ).unwrap();
        assert_eq!(result.width(), 20);
        assert_eq!(result.height(), 20);
    }

    // --- apply_transforms: crop ---

    #[test]
    fn transforms_crop() {
        let png = make_test_png(10, 10);
        let transform = Transform::default();
        let crop = egui::Rect::from_min_size(egui::pos2(2.0, 2.0), Vec2::new(6.0, 4.0));
        let result = apply_transforms(
            &png,
            Vec2::new(10.0, 10.0),
            &transform,
            Some(crop), 1.0, false, false, false,
            1.0,
        ).unwrap();
        assert_eq!(result.width(), 6);
        assert_eq!(result.height(), 4);
    }

    // --- apply_transforms: flip ---

    #[test]
    fn transforms_flip_h() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, image::Rgba([0, 0, 255, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();

        let transform = Transform::default();
        let result = apply_transforms(
            &buf,
            Vec2::new(2.0, 1.0),
            &transform,
            None, 1.0, false, true, false,
            1.0,
        ).unwrap();
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(1, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn transforms_flip_v() {
        let mut img = RgbaImage::new(1, 2);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();

        let transform = Transform::default();
        let result = apply_transforms(
            &buf,
            Vec2::new(1.0, 2.0),
            &transform,
            None, 1.0, false, false, true,
            1.0,
        ).unwrap();
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(0, 1).0, [255, 0, 0, 255]);
    }

    // --- apply_transforms: grayscale ---

    #[test]
    fn transforms_grayscale() {
        let png = make_test_png(4, 4);
        let transform = Transform::default();
        let result = apply_transforms(
            &png,
            Vec2::new(4.0, 4.0),
            &transform,
            None, 1.0, true, false, false,
            1.0,
        ).unwrap();
        let p = result.get_pixel(0, 0).0;
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        assert_eq!(p[3], 255);
    }

    // --- apply_transforms: opacity ---

    #[test]
    fn transforms_opacity_half() {
        let png = make_test_png(4, 4);
        let transform = Transform::default();
        let result = apply_transforms(
            &png,
            Vec2::new(4.0, 4.0),
            &transform,
            None, 0.5, false, false, false,
            1.0,
        ).unwrap();
        let p = result.get_pixel(0, 0).0;
        assert!((p[3] as i16 - 127).unsigned_abs() <= 1);
        assert_eq!(p[0], 255);
    }

    #[test]
    fn transforms_opacity_zero() {
        let png = make_test_png(4, 4);
        let transform = Transform::default();
        let result = apply_transforms(
            &png,
            Vec2::new(4.0, 4.0),
            &transform,
            None, 0.0, false, false, false,
            1.0,
        ).unwrap();
        for pixel in result.pixels() {
            assert_eq!(pixel.0[3], 0);
        }
    }

    // --- apply_transforms: rotation ---

    #[test]
    fn transforms_rotation_preserves_size() {
        let png = make_test_png(10, 10);
        let mut transform = Transform::default();
        transform.rotation = std::f32::consts::FRAC_PI_4;
        let result = apply_transforms(
            &png,
            Vec2::new(10.0, 10.0),
            &transform,
            None, 1.0, false, false, false,
            1.0,
        ).unwrap();
        assert_eq!(result.width(), 10);
        assert_eq!(result.height(), 10);
        let center = result.get_pixel(5, 5).0;
        assert_eq!(center[0], 255);
        assert_eq!(center[3], 255);
        let corner = result.get_pixel(0, 0).0;
        assert_eq!(corner[3], 0);
    }

    // --- apply_transforms: combined ---

    #[test]
    fn transforms_crop_then_scale() {
        let png = make_test_png(20, 20);
        let mut transform = Transform::default();
        transform.scale = Vec2::splat(0.5);
        let crop = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(10.0, 10.0));
        let result = apply_transforms(
            &png,
            Vec2::new(20.0, 20.0),
            &transform,
            Some(crop), 1.0, false, false, false,
            1.0,
        ).unwrap();
        assert_eq!(result.width(), 5);
        assert_eq!(result.height(), 5);
    }

    #[test]
    fn transforms_invalid_image_errors() {
        let transform = Transform::default();
        let result = apply_transforms(
            b"garbage",
            Vec2::new(10.0, 10.0),
            &transform,
            None, 1.0, false, false, false,
            1.0,
        );
        assert!(result.is_err());
    }
}
