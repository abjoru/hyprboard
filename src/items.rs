use std::sync::Arc;

use egui::{Color32, ColorImage, Rect, TextureHandle, Vec2};

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

#[derive(Clone, Debug)]
pub struct Label {
    pub text: String,
    pub offset: Vec2,
    pub font_size: f32,
    pub color: Color32,
}

impl Label {
    pub fn new(text: String, offset: Vec2) -> Self {
        Self {
            text,
            offset,
            font_size: 14.0,
            color: Color32::WHITE,
        }
    }

    pub fn bounding_rect(&self, image_pos: Vec2) -> Rect {
        let pos = image_pos + self.offset;
        let approx_width = (self.text.len() as f32 * self.font_size * 0.6).max(60.0);
        Rect::from_min_size(pos.to_pos2(), Vec2::new(approx_width, self.font_size * 1.4))
    }
}

#[derive(Clone)]
pub struct ImageItem {
    pub texture: Option<TextureHandle>,
    pub original_bytes: Arc<[u8]>,
    pub original_size: Vec2,
    pub transform: Transform,
    pub crop_rect: Option<Rect>,
    pub opacity: f32,
    pub grayscale: bool,
    pub flip_h: bool,
    pub flip_v: bool,
    pub labels: Vec<Label>,
}

#[derive(Clone)]
pub struct TextItem {
    pub content: String,
    pub font_size: f32,
    pub color: Color32,
    pub transform: Transform,
}

#[derive(Clone)]
pub enum BoardItem {
    Image(ImageItem),
    Text(TextItem),
}

impl BoardItem {
    pub fn new_image(
        texture: TextureHandle,
        original_bytes: Arc<[u8]>,
        original_size: Vec2,
        position: Vec2,
    ) -> Self {
        Self::Image(ImageItem {
            texture: Some(texture),
            original_bytes,
            original_size,
            transform: Transform::default().with_position(position),
            crop_rect: None,
            opacity: 1.0,
            grayscale: false,
            flip_h: false,
            flip_v: false,
            labels: Vec::new(),
        })
    }

    pub fn new_text(content: String, position: Vec2) -> Self {
        Self::Text(TextItem {
            content,
            font_size: 16.0,
            color: Color32::WHITE,
            transform: Transform::default().with_position(position),
        })
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
                let approx_width = txt.content.len() as f32 * txt.font_size * 0.6;
                Vec2::new(approx_width, txt.font_size * 1.2)
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

    pub fn labels(&self) -> &[Label] {
        match self {
            Self::Image(img) => &img.labels,
            _ => &[],
        }
    }

    pub fn labels_mut(&mut self) -> Option<&mut Vec<Label>> {
        match self {
            Self::Image(img) => Some(&mut img.labels),
            _ => None,
        }
    }

    pub fn add_label(&mut self, label: Label) {
        if let Self::Image(img) = self {
            img.labels.push(label);
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
        })
    }

    #[test]
    fn text_defaults() {
        let item = BoardItem::new_text("hello".into(), Vec2::new(10.0, 20.0));
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
    fn labels_on_image() {
        let mut item = make_image(Vec2::ZERO, Vec2::new(100.0, 100.0));
        assert_eq!(item.labels().len(), 0);

        item.add_label(Label::new("first".into(), Vec2::new(0.0, -20.0)));
        assert_eq!(item.labels().len(), 1);
        assert_eq!(item.labels()[0].text, "first");

        item.add_label(Label::new("second".into(), Vec2::new(0.0, -40.0)));
        assert_eq!(item.labels().len(), 2);
    }

    #[test]
    fn labels_on_text_item_noop() {
        let mut item = BoardItem::new_text("hi".into(), Vec2::ZERO);
        item.add_label(Label::new("nope".into(), Vec2::ZERO));
        assert_eq!(item.labels().len(), 0);
        assert!(item.labels_mut().is_none());
    }

    #[test]
    fn set_text_content() {
        let mut item = BoardItem::new_text("old".into(), Vec2::ZERO);
        item.set_text_content("new".into());
        assert_eq!(item.text_content().unwrap(), "new");
    }

    #[test]
    fn crop_rect_on_text_is_none() {
        let item = BoardItem::new_text("x".into(), Vec2::ZERO);
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
}
