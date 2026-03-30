use std::sync::Arc;

use egui::{Color32, ColorImage, Rect, TextureHandle, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConnectorId(pub u64);

#[derive(Clone, Debug)]
pub struct Connector {
    pub id: ConnectorId,
    pub from: ItemId,
    pub to: ItemId,
    pub color: Color32,
    pub thickness: f32,
}

impl Connector {
    pub fn new(id: ConnectorId, from: ItemId, to: ItemId) -> Self {
        Self {
            id,
            from,
            to,
            color: Color32::from_gray(180),
            thickness: 2.0,
        }
    }

    /// Compute line endpoints clipped to bounding rect edges.
    pub fn endpoints(&self, items: &[BoardItem]) -> Option<(egui::Pos2, egui::Pos2)> {
        let from_item = items.iter().find(|i| i.item_id() == self.from)?;
        let to_item = items.iter().find(|i| i.item_id() == self.to)?;
        let from_rect = from_item.bounding_rect();
        let to_rect = to_item.bounding_rect();
        let from_center = from_rect.center();
        let to_center = to_rect.center();
        let start = clip_to_rect_edge(from_center, to_center, from_rect);
        let end = clip_to_rect_edge(to_center, from_center, to_rect);
        Some((start, end))
    }

    /// Point-to-segment distance for hit testing.
    pub fn hit_test(&self, point: egui::Pos2, items: &[BoardItem], threshold: f32) -> bool {
        let Some((a, b)) = self.endpoints(items) else {
            return false;
        };
        point_to_segment_dist(point, a, b) <= threshold
    }
}

fn clip_to_rect_edge(from: egui::Pos2, to: egui::Pos2, rect: Rect) -> egui::Pos2 {
    let dir = to - from;
    if dir.length_sq() < 0.001 {
        return from;
    }

    let mut t_max = f32::INFINITY;

    // X slab
    if dir.x.abs() > 0.0001 {
        let t1 = (rect.min.x - from.x) / dir.x;
        let t2 = (rect.max.x - from.x) / dir.x;
        t_max = t_max.min(t1.max(t2));
    }

    // Y slab
    if dir.y.abs() > 0.0001 {
        let t1 = (rect.min.y - from.y) / dir.y;
        let t2 = (rect.max.y - from.y) / dir.y;
        t_max = t_max.min(t1.max(t2));
    }

    // We want the exit point from the rect (t_max)
    let t = t_max.clamp(0.0, 1.0);
    egui::Pos2::new(from.x + dir.x * t, from.y + dir.y * t)
}

fn point_to_segment_dist(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let len_sq = ab.length_sq();
    if len_sq < 0.001 {
        return ap.length();
    }
    let t = (ap.x * ab.x + ap.y * ab.y) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let proj = egui::Pos2::new(a.x + ab.x * t, a.y + ab.y * t);
    (p - proj).length()
}

#[derive(Clone, Debug)]
pub struct Transform {
    pub position: Vec2,
    pub rotation: f32,
    pub scale: Vec2,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::splat(1.0),
        }
    }
}

impl Transform {
    pub fn with_position(mut self, pos: Vec2) -> Self {
        self.position = pos;
        self
    }
}

#[derive(Clone)]
pub struct ImageItem {
    pub id: ItemId,
    pub texture: Option<TextureHandle>,
    pub original_bytes: Arc<[u8]>,
    pub original_size: Vec2,
    pub transform: Transform,
    pub crop_rect: Option<Rect>,
    pub opacity: f32,
    pub grayscale: bool,
    pub flip_h: bool,
    pub flip_v: bool,
    pub border_color: Color32,
}

#[derive(Clone)]
pub struct TextItem {
    pub id: ItemId,
    pub content: String,
    pub font_size: f32,
    pub color: Color32,
    pub bg_color: Color32,
    pub border_color: Color32,
    pub transform: Transform,
    /// Cached rendered size (updated each frame during draw).
    pub cached_size: Vec2,
}

#[derive(Clone)]
pub enum BoardItem {
    Image(ImageItem),
    Text(TextItem),
}

impl BoardItem {
    pub fn new_image(
        id: ItemId,
        texture: TextureHandle,
        original_bytes: Arc<[u8]>,
        original_size: Vec2,
        position: Vec2,
    ) -> Self {
        Self::Image(ImageItem {
            id,
            texture: Some(texture),
            original_bytes,
            original_size,
            transform: Transform::default().with_position(position),
            crop_rect: None,
            opacity: 1.0,
            grayscale: false,
            flip_h: false,
            flip_v: false,
            border_color: Color32::TRANSPARENT,
        })
    }

    pub fn new_text(id: ItemId, content: String, position: Vec2) -> Self {
        Self::Text(TextItem {
            id,
            content,
            font_size: 16.0,
            color: Color32::WHITE,
            bg_color: Color32::TRANSPARENT,
            border_color: Color32::TRANSPARENT,
            transform: Transform::default().with_position(position),
            cached_size: Vec2::ZERO,
        })
    }

    pub fn item_id(&self) -> ItemId {
        match self {
            Self::Image(img) => img.id,
            Self::Text(txt) => txt.id,
        }
    }

    pub fn needs_decode(&self) -> bool {
        matches!(self, Self::Image(img) if img.texture.is_none())
    }

    /// Decode and set texture. Returns true if successful.
    pub fn ensure_texture(&mut self, ctx: &egui::Context, name: &str) -> bool {
        let Self::Image(img) = self else { return false };
        if img.texture.is_some() {
            return true;
        }
        if let Some((tex, size)) =
            decode_and_load_texture(ctx, name, &img.original_bytes, img.grayscale)
        {
            img.original_size = size;
            img.texture = Some(tex);
            return true;
        }
        false
    }

    pub fn transform(&self) -> &Transform {
        match self {
            Self::Image(img) => &img.transform,
            Self::Text(txt) => &txt.transform,
        }
    }

    pub fn transform_mut(&mut self) -> &mut Transform {
        match self {
            Self::Image(img) => &mut img.transform,
            Self::Text(txt) => &mut txt.transform,
        }
    }

    pub fn display_size(&self) -> Vec2 {
        match self {
            Self::Image(img) => {
                let base = img.crop_rect.map(|r| r.size()).unwrap_or(img.original_size);
                base * img.transform.scale
            }
            Self::Text(txt) => {
                if txt.cached_size.x > 0.0 {
                    // Include padding to match render
                    txt.cached_size + Vec2::splat(8.0)
                } else {
                    let line_count = txt.content.lines().count().max(1) as f32;
                    let max_line_len =
                        txt.content.lines().map(|l| l.len()).max().unwrap_or(0) as f32;
                    let approx_width = max_line_len * txt.font_size * 0.6;
                    Vec2::new(approx_width, txt.font_size * 1.4 * line_count)
                }
            }
        }
    }

    pub fn bounding_rect(&self) -> Rect {
        let pos = self.transform().position;
        let size = self.display_size();
        Rect::from_min_size(pos.to_pos2(), size)
    }

    pub fn opacity(&self) -> f32 {
        match self {
            Self::Image(img) => img.opacity,
            Self::Text(_) => 1.0,
        }
    }

    pub fn set_opacity(&mut self, value: f32) {
        if let Self::Image(img) = self {
            img.opacity = value.clamp(0.0, 1.0);
        }
    }

    pub fn crop_rect(&self) -> Option<Rect> {
        match self {
            Self::Image(img) => img.crop_rect,
            _ => None,
        }
    }

    pub fn set_crop_rect(&mut self, rect: Option<Rect>) {
        if let Self::Image(img) = self {
            img.crop_rect = rect;
        }
    }

    pub fn original_size(&self) -> Option<Vec2> {
        match self {
            Self::Image(img) => Some(img.original_size),
            _ => None,
        }
    }

    pub fn text_content(&self) -> Option<&str> {
        match self {
            Self::Text(txt) => Some(&txt.content),
            _ => None,
        }
    }

    pub fn set_text_content(&mut self, new_content: String) {
        if let Self::Text(txt) = self {
            txt.content = new_content;
        }
    }

    pub fn text_font_size(&self) -> Option<f32> {
        match self {
            Self::Text(txt) => Some(txt.font_size),
            _ => None,
        }
    }

    pub fn set_text_font_size(&mut self, size: f32) {
        if let Self::Text(txt) = self {
            txt.font_size = size;
        }
    }

    pub fn text_color(&self) -> Option<Color32> {
        match self {
            Self::Text(txt) => Some(txt.color),
            _ => None,
        }
    }

    pub fn set_text_color(&mut self, color: Color32) {
        if let Self::Text(txt) = self {
            txt.color = color;
        }
    }

    pub fn text_bg_color(&self) -> Option<Color32> {
        match self {
            Self::Text(txt) => Some(txt.bg_color),
            _ => None,
        }
    }

    pub fn set_text_bg_color(&mut self, color: Color32) {
        if let Self::Text(txt) = self {
            txt.bg_color = color;
        }
    }

    pub fn border_color(&self) -> Color32 {
        match self {
            Self::Image(img) => img.border_color,
            Self::Text(txt) => txt.border_color,
        }
    }

    pub fn set_border_color(&mut self, color: Color32) {
        match self {
            Self::Image(img) => img.border_color = color,
            Self::Text(txt) => txt.border_color = color,
        }
    }

    pub fn toggle_flip(&mut self, horizontal: bool) {
        if let Self::Image(img) = self {
            if horizontal {
                img.flip_h = !img.flip_h;
            } else {
                img.flip_v = !img.flip_v;
            }
        }
    }

    pub fn toggle_grayscale(&mut self) {
        let Self::Image(img) = self else { return };
        // Re-decode and rebuild texture (only if already decoded)
        if let Some(tex) = &mut img.texture {
            let Ok(img_data) = image::load_from_memory(&img.original_bytes) else {
                return;
            };
            img.grayscale = !img.grayscale;
            let rgba = if img.grayscale {
                img_data.grayscale().to_rgba8()
            } else {
                img_data.to_rgba8()
            };
            let size = [rgba.width() as usize, rgba.height() as usize];
            let pixels = rgba.into_raw();
            let color_image = ColorImage::from_rgba_unmultiplied(size, &pixels);
            img.original_size = Vec2::new(size[0] as f32, size[1] as f32);
            tex.set(color_image, egui::TextureOptions::LINEAR);
        } else {
            img.grayscale = !img.grayscale;
        }
    }
}

fn decode_and_load_texture(
    ctx: &egui::Context,
    name: &str,
    bytes: &[u8],
    grayscale: bool,
) -> Option<(TextureHandle, Vec2)> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = if grayscale {
        img.grayscale().to_rgba8()
    } else {
        img.to_rgba8()
    };
    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels = rgba.into_raw();
    let color_image = ColorImage::from_rgba_unmultiplied(size, &pixels);
    let original_size = Vec2::new(size[0] as f32, size[1] as f32);
    let texture = ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR);
    Some((texture, original_size))
}

pub fn load_image_from_bytes(
    ctx: &egui::Context,
    name: &str,
    bytes: &[u8],
) -> Option<(TextureHandle, Vec2, Arc<[u8]>)> {
    let (texture, original_size) = decode_and_load_texture(ctx, name, bytes, false)?;
    Some((texture, original_size, Arc::from(bytes)))
}

/// Get image dimensions without full decode
pub fn image_dimensions(bytes: &[u8]) -> Option<Vec2> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    Some(Vec2::new(w as f32, h as f32))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn make_image(pos: Vec2, size: Vec2) -> BoardItem {
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
    fn text_defaults() {
        let item = BoardItem::new_text(ItemId(0), "hello".into(), Vec2::new(10.0, 20.0));
        assert_eq!(item.text_content().unwrap(), "hello");
        assert_eq!(item.transform().position, Vec2::new(10.0, 20.0));
        assert_eq!(item.transform().scale, Vec2::splat(1.0));
        assert_eq!(item.opacity(), 1.0);
        assert!(!item.needs_decode());
    }

    #[test]
    fn image_defaults() {
        let item = make_image(Vec2::new(5.0, 5.0), Vec2::new(200.0, 100.0));
        assert_eq!(item.transform().position, Vec2::new(5.0, 5.0));
        assert_eq!(item.opacity(), 1.0);
        assert!(item.crop_rect().is_none());
        assert_eq!(item.original_size(), Some(Vec2::new(200.0, 100.0)));
        assert!(item.needs_decode()); // texture is None
    }

    #[test]
    fn display_size_no_crop() {
        let item = make_image(Vec2::ZERO, Vec2::new(200.0, 100.0));
        assert_eq!(item.display_size(), Vec2::new(200.0, 100.0));
    }

    #[test]
    fn display_size_with_crop() {
        let mut item = make_image(Vec2::ZERO, Vec2::new(200.0, 100.0));
        item.set_crop_rect(Some(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            Vec2::new(80.0, 60.0),
        )));
        assert_eq!(item.display_size(), Vec2::new(80.0, 60.0));
    }

    #[test]
    fn display_size_with_scale() {
        let mut item = make_image(Vec2::ZERO, Vec2::new(100.0, 100.0));
        item.transform_mut().scale = Vec2::splat(2.0);
        assert_eq!(item.display_size(), Vec2::new(200.0, 200.0));
    }

    #[test]
    fn bounding_rect_position_and_size() {
        let item = make_image(Vec2::new(10.0, 20.0), Vec2::new(100.0, 50.0));
        let rect = item.bounding_rect();
        assert_eq!(rect.min, egui::pos2(10.0, 20.0));
        assert_eq!(rect.size(), Vec2::new(100.0, 50.0));
    }

    #[test]
    fn set_opacity_clamps() {
        let mut item = make_image(Vec2::ZERO, Vec2::new(10.0, 10.0));
        item.set_opacity(1.5);
        assert_eq!(item.opacity(), 1.0);
        item.set_opacity(-0.5);
        assert_eq!(item.opacity(), 0.0);
        item.set_opacity(0.7);
        assert!((item.opacity() - 0.7).abs() < 0.001);
    }

    #[test]
    fn toggle_flip() {
        let mut item = make_image(Vec2::ZERO, Vec2::new(10.0, 10.0));
        assert!(matches!(&item, BoardItem::Image(img) if !img.flip_h && !img.flip_v));

        item.toggle_flip(true);
        assert!(matches!(&item, BoardItem::Image(img) if img.flip_h && !img.flip_v));

        item.toggle_flip(false);
        assert!(matches!(&item, BoardItem::Image(img) if img.flip_h && img.flip_v));

        item.toggle_flip(true);
        assert!(matches!(&item, BoardItem::Image(img) if !img.flip_h && img.flip_v));
    }

    #[test]
    fn toggle_grayscale_no_texture() {
        let mut item = make_image(Vec2::ZERO, Vec2::new(10.0, 10.0));
        assert!(matches!(&item, BoardItem::Image(img) if !img.grayscale));

        item.toggle_grayscale();
        assert!(matches!(&item, BoardItem::Image(img) if img.grayscale));

        item.toggle_grayscale();
        assert!(matches!(&item, BoardItem::Image(img) if !img.grayscale));
    }

    #[test]
    fn set_text_content() {
        let mut item = BoardItem::new_text(ItemId(0), "old".into(), Vec2::ZERO);
        item.set_text_content("new".into());
        assert_eq!(item.text_content().unwrap(), "new");
    }

    #[test]
    fn crop_rect_on_text_is_none() {
        let item = BoardItem::new_text(ItemId(0), "x".into(), Vec2::ZERO);
        assert!(item.crop_rect().is_none());
        assert!(item.original_size().is_none());
    }

    #[test]
    fn image_dimensions_valid_png() {
        // Minimal 1x1 PNG
        let png = {
            let mut buf = Vec::new();
            let encoder = image::codecs::png::PngEncoder::new(&mut buf);
            image::ImageEncoder::write_image(
                encoder,
                &[255, 0, 0, 255],
                1,
                1,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
            buf
        };
        let dims = image_dimensions(&png).unwrap();
        assert_eq!(dims, Vec2::new(1.0, 1.0));
    }

    #[test]
    fn image_dimensions_invalid() {
        assert!(image_dimensions(&[0, 1, 2, 3]).is_none());
    }

    fn make_image_with_id(id: u64, pos: Vec2, size: Vec2) -> BoardItem {
        BoardItem::Image(ImageItem {
            id: ItemId(id),
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
    fn connector_endpoints() {
        let items = vec![
            make_image_with_id(1, Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0)),
            make_image_with_id(2, Vec2::new(200.0, 0.0), Vec2::new(100.0, 100.0)),
        ];
        let conn = Connector::new(ConnectorId(1), ItemId(1), ItemId(2));
        let (start, end) = conn.endpoints(&items).unwrap();
        // Start should be on right edge of first item (100, 50)
        assert!((start.x - 100.0).abs() < 1.0);
        // End should be on left edge of second item (200, 50)
        assert!((end.x - 200.0).abs() < 1.0);
    }

    #[test]
    fn connector_endpoints_missing_item() {
        let items = vec![make_image(Vec2::ZERO, Vec2::new(100.0, 100.0))];
        let conn = Connector::new(ConnectorId(1), ItemId(0), ItemId(99));
        assert!(conn.endpoints(&items).is_none());
    }

    #[test]
    fn connector_hit_test() {
        let items = vec![
            make_image_with_id(1, Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0)),
            make_image_with_id(2, Vec2::new(200.0, 0.0), Vec2::new(100.0, 100.0)),
        ];
        let conn = Connector::new(ConnectorId(1), ItemId(1), ItemId(2));
        // Point on the line between items (midpoint, y=50)
        assert!(conn.hit_test(egui::pos2(150.0, 50.0), &items, 5.0));
        // Point far away
        assert!(!conn.hit_test(egui::pos2(150.0, 200.0), &items, 5.0));
    }

    #[test]
    fn point_to_segment_distance() {
        let a = egui::pos2(0.0, 0.0);
        let b = egui::pos2(10.0, 0.0);
        // Point directly above midpoint
        assert!((point_to_segment_dist(egui::pos2(5.0, 3.0), a, b) - 3.0).abs() < 0.01);
        // Point at endpoint
        assert!(point_to_segment_dist(egui::pos2(0.0, 0.0), a, b) < 0.01);
        // Point beyond segment end
        assert!((point_to_segment_dist(egui::pos2(15.0, 0.0), a, b) - 5.0).abs() < 0.01);
    }
}
