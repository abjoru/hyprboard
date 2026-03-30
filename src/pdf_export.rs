use ::image::RgbaImage;
// ImageEncoder trait needed for JpegEncoder::write_image()
use ::image::ImageEncoder as _;
use printpdf::{
    BuiltinFont, Color, ColorBits, ColorSpace, Image, ImageFilter, ImageTransform, ImageXObject,
    IndirectFontRef, Mm, PdfDocument, PdfLayerReference, Px, Rgb,
};

use crate::clipboard::apply_transforms;
use crate::items::BoardItem;

/// Page size presets.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum PageSize {
    #[default]
    A4,
    Letter,
}

impl PageSize {
    /// (width, height) in mm — portrait orientation.
    fn dimensions_mm(self) -> (f32, f32) {
        match self {
            Self::A4 => (210.0, 297.0),
            Self::Letter => (215.9, 279.4),
        }
    }

    pub const ALL: [PageSize; 2] = [Self::A4, Self::Letter];

    pub fn label(self) -> &'static str {
        match self {
            Self::A4 => "A4",
            Self::Letter => "Letter",
        }
    }
}

/// PDF export mode.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum PdfMode {
    /// Scale all items to fit a single page.
    #[default]
    SinglePage,
    /// Dense row-packing across multiple pages.
    MultiPage,
}

impl PdfMode {
    pub const ALL: [PdfMode; 2] = [Self::SinglePage, Self::MultiPage];

    pub fn label(self) -> &'static str {
        match self {
            Self::SinglePage => "Single Page",
            Self::MultiPage => "Multi Page",
        }
    }
}

const MARGIN_MM: f32 = 10.0;
/// Convert scene pixels to mm assuming 96 DPI.
const PX_TO_MM: f32 = 25.4 / 96.0;

/// Export all board items to PDF bytes.
pub fn export_pdf(
    items: &[BoardItem],
    mode: PdfMode,
    page_size: PageSize,
) -> Result<Vec<u8>, String> {
    if items.is_empty() {
        return Err("no items to export".into());
    }

    match mode {
        PdfMode::SinglePage => export_single_page(items, page_size),
        PdfMode::MultiPage => export_multi_page(items, page_size),
    }
}

// ---------------------------------------------------------------------------
// Single page: scale everything to fit
// ---------------------------------------------------------------------------

fn export_single_page(items: &[BoardItem], page_size: PageSize) -> Result<Vec<u8>, String> {
    let (pw, ph) = page_size.dimensions_mm();
    let content_w = pw - 2.0 * MARGIN_MM;
    let content_h = ph - 2.0 * MARGIN_MM;

    let union = items_bounding_rect(items);
    let board_w = union.width();
    let board_h = union.height();

    if board_w <= 0.0 || board_h <= 0.0 {
        return Err("zero-size board".into());
    }

    let scale = (content_w / board_w).min(content_h / board_h).min(1.0);

    let (doc, page, layer) = PdfDocument::new("HyprBoard Export", Mm(pw), Mm(ph), "Content");
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| e.to_string())?;
    let _bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(|e| e.to_string())?;
    let layer_ref = doc.get_page(page).get_layer(layer);

    let scaled_w = board_w * scale;
    let scaled_h = board_h * scale;
    let offset_x = MARGIN_MM + (content_w - scaled_w) / 2.0;
    let offset_y = MARGIN_MM + (content_h - scaled_h) / 2.0;

    for item in items {
        let rect = item.bounding_rect();
        let rel_x = (rect.min.x - union.min.x) * scale;
        let rel_y = (rect.min.y - union.min.y) * scale;
        let item_w = rect.width() * scale;
        let item_h = rect.height() * scale;

        // Convert top-left coords to PDF bottom-left coords
        let pdf_x = offset_x + rel_x;
        let pdf_y = ph - offset_y - rel_y - item_h;

        draw_item(&layer_ref, &font, item, pdf_x, pdf_y, item_w, item_h)?;
    }

    doc.save_to_bytes().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Multi page: dense row packing
// ---------------------------------------------------------------------------

fn export_multi_page(items: &[BoardItem], page_size: PageSize) -> Result<Vec<u8>, String> {
    let (pw, ph) = page_size.dimensions_mm();
    let content_w = pw - 2.0 * MARGIN_MM;
    let content_h = ph - 2.0 * MARGIN_MM;

    // Collect items with display sizes converted from scene pixels to mm
    let mut sized: Vec<(usize, f32, f32)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let sz = item.display_size();
        let (mut w, mut h) = (sz.x * PX_TO_MM, sz.y * PX_TO_MM);
        if w > content_w {
            let s = content_w / w;
            w *= s;
            h *= s;
        }
        if h > content_h {
            let s = content_h / h;
            w *= s;
            h *= s;
        }
        sized.push((i, w, h));
    }

    // Sort top-to-bottom, left-to-right by original position
    sized.sort_by(|a, b| {
        let ra = items[a.0].bounding_rect();
        let rb = items[b.0].bounding_rect();
        ra.min
            .y
            .total_cmp(&rb.min.y)
            .then(ra.min.x.total_cmp(&rb.min.x))
    });

    // Row-pack into pages: (item_idx, x, y, w, h)
    type PageLayout = Vec<(usize, f32, f32, f32, f32)>;
    let gap = 4.0_f32;
    let mut pages: Vec<PageLayout> = Vec::new();
    let mut cursor_x = 0.0_f32;
    let mut cursor_y = 0.0_f32;
    let mut row_height = 0.0_f32;
    let mut current_page: PageLayout = Vec::new();

    for &(idx, w, h) in &sized {
        if cursor_x + w > content_w && cursor_x > 0.0 {
            cursor_y += row_height + gap;
            cursor_x = 0.0;
            row_height = 0.0;
        }

        if cursor_y + h > content_h && !current_page.is_empty() {
            pages.push(std::mem::take(&mut current_page));
            cursor_x = 0.0;
            cursor_y = 0.0;
            row_height = 0.0;
        }

        current_page.push((idx, cursor_x, cursor_y, w, h));
        cursor_x += w + gap;
        row_height = row_height.max(h);
    }
    if !current_page.is_empty() {
        pages.push(current_page);
    }

    if pages.is_empty() {
        return Err("no pages generated".into());
    }

    let (doc, first_page, first_layer) =
        PdfDocument::new("HyprBoard Export", Mm(pw), Mm(ph), "Content");
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| e.to_string())?;
    let _bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(|e| e.to_string())?;

    for (page_idx, page_items) in pages.iter().enumerate() {
        let (page_ref, layer_idx) = if page_idx == 0 {
            (first_page, first_layer)
        } else {
            doc.add_page(Mm(pw), Mm(ph), "Content")
        };
        let layer_ref = doc.get_page(page_ref).get_layer(layer_idx);

        for &(item_idx, x, y, w, h) in page_items {
            let pdf_x = MARGIN_MM + x;
            let pdf_y = ph - MARGIN_MM - y - h;
            draw_item(&layer_ref, &font, &items[item_idx], pdf_x, pdf_y, w, h)?;
        }
    }

    doc.save_to_bytes().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn items_bounding_rect(items: &[BoardItem]) -> egui::Rect {
    let mut r = items[0].bounding_rect();
    for item in &items[1..] {
        r = r.union(item.bounding_rect());
    }
    r
}

#[allow(clippy::too_many_arguments)]
fn draw_item(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    item: &BoardItem,
    pdf_x: f32,
    pdf_y: f32,
    target_w: f32,
    target_h: f32,
) -> Result<(), String> {
    match item {
        BoardItem::Image(img) => {
            let rgba = apply_transforms(
                &img.original_bytes,
                img.original_size,
                &img.transform,
                img.crop_rect,
                img.opacity,
                img.grayscale,
                img.flip_h,
                img.flip_v,
                1.0,
            )?;

            embed_image(layer, &rgba, pdf_x, pdf_y, target_w, target_h)?;
        }
        BoardItem::Text(txt) => {
            draw_text(
                layer,
                font,
                &txt.content,
                txt.font_size,
                txt.color,
                pdf_x,
                pdf_y,
            );
        }
    }
    Ok(())
}

/// Composite RGBA onto white, encode as JPEG, embed in PDF.
fn embed_image(
    layer: &PdfLayerReference,
    rgba: &RgbaImage,
    pdf_x: f32,
    pdf_y: f32,
    target_w: f32,
    target_h: f32,
) -> Result<(), String> {
    let w = rgba.width() as usize;
    let h = rgba.height() as usize;
    if w == 0 || h == 0 {
        return Ok(());
    }

    // Composite onto white background → RGB
    let mut rgb_data = Vec::with_capacity(w * h * 3);
    for pixel in rgba.pixels() {
        let [r, g, b, a] = pixel.0;
        let af = a as f32 / 255.0;
        let bg = 255.0;
        rgb_data.push(((r as f32 * af + bg * (1.0 - af)).round()) as u8);
        rgb_data.push(((g as f32 * af + bg * (1.0 - af)).round()) as u8);
        rgb_data.push(((b as f32 * af + bg * (1.0 - af)).round()) as u8);
    }

    // Encode as JPEG
    let mut jpeg_buf = Vec::new();
    {
        let encoder = ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, 85);
        encoder
            .write_image(
                &rgb_data,
                w as u32,
                h as u32,
                ::image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| format!("JPEG encode: {e}"))?;
    }

    let xobj = ImageXObject {
        width: Px(w),
        height: Px(h),
        color_space: ColorSpace::Rgb,
        bits_per_component: ColorBits::Bit8,
        interpolate: true,
        image_data: jpeg_buf,
        image_filter: Some(ImageFilter::DCT),
        smask: None,
        clipping_bbox: None,
    };

    let image = Image::from(xobj);
    let dpi_x = (w as f32 / target_w) * 25.4;
    let dpi_y = (h as f32 / target_h) * 25.4;
    let scale_y = if dpi_y.abs() > 0.001 {
        dpi_x / dpi_y
    } else {
        1.0
    };

    image.add_to_layer(
        layer.clone(),
        ImageTransform {
            translate_x: Some(Mm(pdf_x)),
            translate_y: Some(Mm(pdf_y)),
            dpi: Some(dpi_x),
            scale_x: None,
            scale_y: Some(scale_y),
            rotate: None,
        },
    );

    Ok(())
}

fn draw_text(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    text: &str,
    font_size: f32,
    color: egui::Color32,
    pdf_x: f32,
    pdf_y: f32,
) {
    if text.is_empty() {
        return;
    }
    let r = color.r() as f32 / 255.0;
    let g = color.g() as f32 / 255.0;
    let b = color.b() as f32 / 255.0;

    layer.begin_text_section();
    layer.set_fill_color(Color::Rgb(Rgb::new(r, g, b, None)));
    layer.set_font(font, font_size);
    layer.set_text_cursor(Mm(pdf_x), Mm(pdf_y));

    // Filter to Latin-1 safe chars (builtin fonts limitation)
    let safe: String = text
        .chars()
        .map(|c| if (c as u32) <= 255 { c } else { '?' })
        .collect();
    layer.write_text(&safe, font);
    layer.end_text_section();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::{ImageItem, ItemId, TextItem, Transform};
    use std::sync::Arc;

    fn test_png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::new(w, h);
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            ::image::ImageFormat::Png,
        )
        .unwrap();
        buf
    }

    fn make_image(x: f32, y: f32, w: f32, h: f32) -> BoardItem {
        BoardItem::Image(ImageItem {
            id: ItemId(0),
            texture: None,
            original_bytes: Arc::from(test_png(w as u32, h as u32)),
            original_size: egui::Vec2::new(w, h),
            transform: Transform::default().with_position(egui::Vec2::new(x, y)),
            crop_rect: None,
            opacity: 1.0,
            grayscale: false,
            flip_h: false,
            flip_v: false,
            border_color: egui::Color32::TRANSPARENT,
        })
    }

    #[test]
    fn single_page_basic() {
        let items = vec![make_image(0.0, 0.0, 100.0, 100.0)];
        let bytes = export_pdf(&items, PdfMode::SinglePage, PageSize::A4).unwrap();
        assert!(bytes.len() > 100);
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn multi_page_basic() {
        let items = vec![make_image(0.0, 0.0, 100.0, 100.0)];
        let bytes = export_pdf(&items, PdfMode::MultiPage, PageSize::Letter).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn text_items_exported() {
        let items = vec![BoardItem::Text(TextItem {
            id: ItemId(0),
            content: "Hello PDF".into(),
            font_size: 16.0,
            color: egui::Color32::WHITE,
            bg_color: egui::Color32::TRANSPARENT,
            border_color: egui::Color32::TRANSPARENT,
            transform: Transform::default().with_position(egui::Vec2::new(10.0, 20.0)),
            cached_size: egui::Vec2::ZERO,
        })];
        let bytes = export_pdf(&items, PdfMode::SinglePage, PageSize::A4).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn mixed_items() {
        let items = vec![
            make_image(0.0, 0.0, 200.0, 150.0),
            BoardItem::Text(TextItem {
                id: ItemId(0),
                content: "Caption".into(),
                font_size: 12.0,
                color: egui::Color32::BLACK,
                bg_color: egui::Color32::TRANSPARENT,
                border_color: egui::Color32::TRANSPARENT,
                transform: Transform::default().with_position(egui::Vec2::new(0.0, 160.0)),
                cached_size: egui::Vec2::ZERO,
            }),
            make_image(220.0, 0.0, 300.0, 200.0),
        ];
        let bytes = export_pdf(&items, PdfMode::MultiPage, PageSize::A4).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn empty_items_error() {
        let result = export_pdf(&[], PdfMode::SinglePage, PageSize::A4);
        assert!(result.is_err());
    }
}
