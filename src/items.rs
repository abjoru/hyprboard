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
        if let Some((tex, size)) = decode_and_load_texture(ctx, name, &img.original_bytes, img.grayscale) {
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
                let base = img.crop_rect
                    .map(|r| r.size())
                    .unwrap_or(img.original_size);
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
        img.grayscale = !img.grayscale;
        // Re-decode and rebuild texture (only if already decoded)
        if let Some(tex) = &mut img.texture {
            if let Some(img_data) = image::load_from_memory(&img.original_bytes).ok() {
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
            }
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
