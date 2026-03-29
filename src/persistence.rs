use std::path::Path;
use std::sync::Arc;

use egui::{Color32, Vec2};
use rusqlite::{Connection, params};

fn color_to_u32(c: Color32) -> u32 {
    u32::from_be_bytes([c.r(), c.g(), c.b(), c.a()])
}

fn u32_to_color(v: u32) -> Color32 {
    let [r, g, b, a] = v.to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

use crate::items::{BoardItem, ImageItem, Label, TextItem, Transform, image_dimensions};

const SCHEMA_VERSION: &str = "3";

fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS items (
            id         INTEGER PRIMARY KEY,
            z_order    INTEGER NOT NULL,
            item_type  TEXT NOT NULL,
            pos_x      REAL NOT NULL,
            pos_y      REAL NOT NULL,
            scale_x    REAL NOT NULL DEFAULT 1.0,
            scale_y    REAL NOT NULL DEFAULT 1.0,
            rotation   REAL NOT NULL DEFAULT 0.0,
            image_data BLOB,
            crop_x     REAL,
            crop_y     REAL,
            crop_w     REAL,
            crop_h     REAL,
            opacity    REAL DEFAULT 1.0,
            grayscale  INTEGER DEFAULT 0,
            flip_h     INTEGER DEFAULT 0,
            flip_v     INTEGER DEFAULT 0,
            content    TEXT,
            font_size  REAL,
            color      INTEGER,
            img_width  REAL,
            img_height REAL,
            bg_color   INTEGER,
            border_color INTEGER
        );
        CREATE TABLE IF NOT EXISTS labels (
            id         INTEGER PRIMARY KEY,
            item_id    INTEGER NOT NULL REFERENCES items(id),
            text       TEXT NOT NULL,
            offset_x   REAL NOT NULL DEFAULT 0.0,
            offset_y   REAL NOT NULL DEFAULT 0.0,
            font_size  REAL NOT NULL DEFAULT 14.0,
            color      INTEGER NOT NULL DEFAULT 4294967295
        );",
    )
}

fn migrate_schema(conn: &Connection) -> rusqlite::Result<()> {
    // Check if img_width column exists
    let has_img_width: bool = conn.prepare("SELECT img_width FROM items LIMIT 0").is_ok();

    if !has_img_width {
        conn.execute_batch(
            "ALTER TABLE items ADD COLUMN img_width REAL;
             ALTER TABLE items ADD COLUMN img_height REAL;",
        )?;
        log::info!("Migrated schema: added img_width/img_height columns");
    }

    let has_bg_color: bool = conn.prepare("SELECT bg_color FROM items LIMIT 0").is_ok();
    if !has_bg_color {
        conn.execute_batch(
            "ALTER TABLE items ADD COLUMN bg_color INTEGER;
             ALTER TABLE items ADD COLUMN border_color INTEGER;",
        )?;
        log::info!("Migrated schema: added bg_color/border_color columns");
    }

    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        params!["version", SCHEMA_VERSION],
    )?;

    Ok(())
}

pub fn save_board(path: &Path, items: &[BoardItem]) -> rusqlite::Result<()> {
    // Write to temp file then rename for atomicity
    let tmp_path = path.with_extension("hboard.tmp");
    {
        let conn = Connection::open(&tmp_path)?;
        conn.execute_batch("PRAGMA journal_mode=OFF;")?;
        create_schema(&conn)?;

        conn.execute("DELETE FROM labels", [])?;
        conn.execute("DELETE FROM items", [])?;
        conn.execute("DELETE FROM meta", [])?;
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params!["version", SCHEMA_VERSION],
        )?;

        let mut stmt = conn.prepare(
            "INSERT INTO items (
                z_order, item_type, pos_x, pos_y, scale_x, scale_y, rotation,
                image_data, crop_x, crop_y, crop_w, crop_h,
                opacity, grayscale, flip_h, flip_v,
                content, font_size, color,
                img_width, img_height,
                bg_color, border_color
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16,
                ?17, ?18, ?19,
                ?20, ?21,
                ?22, ?23
            )",
        )?;

        let mut label_stmt = conn.prepare(
            "INSERT INTO labels (item_id, text, offset_x, offset_y, font_size, color)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for (z_order, item) in items.iter().enumerate() {
            match item {
                BoardItem::Image(img) => {
                    let (cx, cy, cw, ch) = img
                        .crop_rect
                        .map(|r| {
                            (
                                Some(r.min.x as f64),
                                Some(r.min.y as f64),
                                Some(r.width() as f64),
                                Some(r.height() as f64),
                            )
                        })
                        .unwrap_or((None, None, None, None));

                    stmt.execute(params![
                        z_order as i64,
                        "image",
                        img.transform.position.x as f64,
                        img.transform.position.y as f64,
                        img.transform.scale.x as f64,
                        img.transform.scale.y as f64,
                        img.transform.rotation as f64,
                        &*img.original_bytes,
                        cx,
                        cy,
                        cw,
                        ch,
                        img.opacity as f64,
                        img.grayscale as i32,
                        img.flip_h as i32,
                        img.flip_v as i32,
                        None::<String>,
                        None::<f64>,
                        None::<i64>,
                        img.original_size.x as f64,
                        img.original_size.y as f64,
                        None::<i64>,
                        color_to_u32(img.border_color) as i64,
                    ])?;

                    let item_id = conn.last_insert_rowid();
                    for label in &img.labels {
                        label_stmt.execute(params![
                            item_id,
                            label.text,
                            label.offset.x as f64,
                            label.offset.y as f64,
                            label.font_size as f64,
                            color_to_u32(label.color) as i64,
                        ])?;
                    }
                }
                BoardItem::Text(txt) => {
                    stmt.execute(params![
                        z_order as i64,
                        "text",
                        txt.transform.position.x as f64,
                        txt.transform.position.y as f64,
                        txt.transform.scale.x as f64,
                        txt.transform.scale.y as f64,
                        txt.transform.rotation as f64,
                        None::<Vec<u8>>,
                        None::<f64>,
                        None::<f64>,
                        None::<f64>,
                        None::<f64>,
                        None::<f64>,
                        None::<i32>,
                        None::<i32>,
                        None::<i32>,
                        &txt.content,
                        txt.font_size as f64,
                        color_to_u32(txt.color),
                        None::<f64>,
                        None::<f64>,
                        color_to_u32(txt.bg_color) as i64,
                        color_to_u32(txt.border_color) as i64,
                    ])?;
                }
            }
        }
    }

    std::fs::rename(&tmp_path, path)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    Ok(())
}

pub fn load_board(path: &Path) -> rusqlite::Result<Vec<BoardItem>> {
    let conn = Connection::open(path)?;

    // Migrate if needed
    migrate_schema(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT id, item_type, pos_x, pos_y, scale_x, scale_y, rotation,
                image_data, crop_x, crop_y, crop_w, crop_h,
                opacity, grayscale, flip_h, flip_v,
                content, font_size, color,
                img_width, img_height,
                bg_color, border_color
         FROM items ORDER BY z_order ASC",
    )?;

    let mut label_stmt = conn.prepare(
        "SELECT text, offset_x, offset_y, font_size, color
         FROM labels WHERE item_id = ?1",
    )?;

    let mut items = Vec::new();

    let rows = stmt.query_map([], |row| {
        Ok(RawRow {
            id: row.get(0)?,
            item_type: row.get(1)?,
            pos_x: row.get(2)?,
            pos_y: row.get(3)?,
            scale_x: row.get(4)?,
            scale_y: row.get(5)?,
            rotation: row.get(6)?,
            image_data: row.get(7)?,
            crop_x: row.get(8)?,
            crop_y: row.get(9)?,
            crop_w: row.get(10)?,
            crop_h: row.get(11)?,
            opacity: row.get(12)?,
            grayscale: row.get(13)?,
            flip_h: row.get(14)?,
            flip_v: row.get(15)?,
            content: row.get(16)?,
            font_size: row.get(17)?,
            color: row.get(18)?,
            img_width: row.get(19)?,
            img_height: row.get(20)?,
            bg_color: row.get(21)?,
            border_color: row.get(22)?,
        })
    })?;

    for row in rows {
        let row = row?;
        let transform = Transform {
            position: Vec2::new(row.pos_x as f32, row.pos_y as f32),
            rotation: row.rotation as f32,
            scale: Vec2::new(row.scale_x as f32, row.scale_y as f32),
        };

        match row.item_type.as_str() {
            "image" => {
                let Some(bytes) = row.image_data else {
                    continue;
                };

                // Use stored dimensions if available, else probe
                let original_size = match (row.img_width, row.img_height) {
                    (Some(w), Some(h)) => Vec2::new(w as f32, h as f32),
                    _ => image_dimensions(&bytes).unwrap_or(Vec2::new(100.0, 100.0)),
                };

                let crop_rect = match (row.crop_x, row.crop_y, row.crop_w, row.crop_h) {
                    (Some(x), Some(y), Some(w), Some(h)) => Some(egui::Rect::from_min_size(
                        egui::pos2(x as f32, y as f32),
                        Vec2::new(w as f32, h as f32),
                    )),
                    _ => None,
                };

                // Load labels for this item
                let labels: Vec<Label> = label_stmt
                    .query_map(params![row.id], |lrow| {
                        let text: String = lrow.get(0)?;
                        let offset_x: f64 = lrow.get(1)?;
                        let offset_y: f64 = lrow.get(2)?;
                        let font_size: f64 = lrow.get(3)?;
                        let color_val: i64 = lrow.get(4)?;
                        Ok((text, offset_x, offset_y, font_size, color_val))
                    })?
                    .filter_map(|r| r.ok())
                    .map(|(text, ox, oy, fs, cv)| Label {
                        text,
                        offset: Vec2::new(ox as f32, oy as f32),
                        font_size: fs as f32,
                        color: u32_to_color(cv as u32),
                    })
                    .collect();

                // Lazy load: don't decode, just store bytes + dimensions
                let border_color = u32_to_color(row.border_color.unwrap_or(0) as u32);

                items.push(BoardItem::Image(ImageItem {
                    texture: None,
                    original_bytes: Arc::from(bytes),
                    original_size,
                    transform,
                    crop_rect,
                    opacity: row.opacity.unwrap_or(1.0) as f32,
                    grayscale: row.grayscale.unwrap_or(0) != 0,
                    flip_h: row.flip_h.unwrap_or(0) != 0,
                    flip_v: row.flip_v.unwrap_or(0) != 0,
                    labels,
                    border_color,
                }));
            }
            "text" => {
                let content = row.content.unwrap_or_default();
                let font_size = row.font_size.unwrap_or(16.0) as f32;
                let color = u32_to_color(row.color.unwrap_or(0xFFFFFFFF) as u32);
                let bg_color = u32_to_color(row.bg_color.unwrap_or(0) as u32);
                let border_color = u32_to_color(row.border_color.unwrap_or(0) as u32);

                items.push(BoardItem::Text(TextItem {
                    content,
                    font_size,
                    color,
                    bg_color,
                    border_color,
                    transform,
                }));
            }
            _ => {}
        }
    }

    Ok(items)
}

struct RawRow {
    id: i64,
    item_type: String,
    pos_x: f64,
    pos_y: f64,
    scale_x: f64,
    scale_y: f64,
    rotation: f64,
    image_data: Option<Vec<u8>>,
    crop_x: Option<f64>,
    crop_y: Option<f64>,
    crop_w: Option<f64>,
    crop_h: Option<f64>,
    opacity: Option<f64>,
    grayscale: Option<i32>,
    flip_h: Option<i32>,
    flip_v: Option<i32>,
    content: Option<String>,
    font_size: Option<f64>,
    color: Option<i64>,
    img_width: Option<f64>,
    img_height: Option<f64>,
    bg_color: Option<i64>,
    border_color: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn make_test_png(w: u32, h: u32) -> Vec<u8> {
        let mut img = RgbaImage::new(w, h);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([255, 0, 0, 255]);
        }
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn round_trip_image_item() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_roundtrip_image.hboard");

        let png = make_test_png(10, 10);
        let items = vec![BoardItem::Image(ImageItem {
            texture: None,
            original_bytes: Arc::from(png.as_slice()),
            original_size: Vec2::new(10.0, 10.0),
            transform: Transform {
                position: Vec2::new(42.0, 84.0),
                rotation: 1.5,
                scale: Vec2::new(2.0, 3.0),
            },
            crop_rect: Some(egui::Rect::from_min_size(
                egui::pos2(1.0, 2.0),
                Vec2::new(5.0, 6.0),
            )),
            opacity: 0.7,
            grayscale: true,
            flip_h: true,
            flip_v: false,
            labels: vec![Label {
                text: "test label".into(),
                offset: Vec2::new(10.0, -20.0),
                font_size: 18.0,
                color: egui::Color32::from_rgb(255, 128, 0),
            }],
            border_color: egui::Color32::TRANSPARENT,
        })];

        save_board(&path, &items).unwrap();

        let loaded = load_board(&path).unwrap();

        assert_eq!(loaded.len(), 1);
        let BoardItem::Image(img) = &loaded[0] else {
            panic!("expected Image");
        };
        assert_eq!(img.transform.position, Vec2::new(42.0, 84.0));
        assert!((img.transform.rotation - 1.5).abs() < 0.01);
        assert_eq!(img.transform.scale, Vec2::new(2.0, 3.0));
        assert!(img.crop_rect.is_some());
        let cr = img.crop_rect.unwrap();
        assert!((cr.min.x - 1.0).abs() < 0.01);
        assert!((cr.min.y - 2.0).abs() < 0.01);
        assert!((cr.width() - 5.0).abs() < 0.01);
        assert!((img.opacity - 0.7).abs() < 0.01);
        assert!(img.grayscale);
        assert!(img.flip_h);
        assert!(!img.flip_v);
        assert_eq!(img.labels.len(), 1);
        assert_eq!(img.labels[0].text, "test label");
        assert!((img.labels[0].offset.x - 10.0).abs() < 0.01);
        assert!((img.labels[0].font_size - 18.0).abs() < 0.01);
        assert_eq!(&img.original_bytes[..], &png[..]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn round_trip_text_item() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_roundtrip_text.hboard");

        let items = vec![BoardItem::Text(TextItem {
            content: "hello world".into(),
            font_size: 24.0,
            color: egui::Color32::from_rgb(0, 255, 128),
            bg_color: egui::Color32::TRANSPARENT,
            border_color: egui::Color32::TRANSPARENT,
            transform: Transform {
                position: Vec2::new(100.0, 200.0),
                rotation: 0.0,
                scale: Vec2::splat(1.0),
            },
        })];

        save_board(&path, &items).unwrap();

        let loaded = load_board(&path).unwrap();

        assert_eq!(loaded.len(), 1);
        let BoardItem::Text(txt) = &loaded[0] else {
            panic!("expected Text");
        };
        assert_eq!(txt.content, "hello world");
        assert!((txt.font_size - 24.0).abs() < 0.01);
        assert_eq!(txt.transform.position, Vec2::new(100.0, 200.0));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn round_trip_mixed_items() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_roundtrip_mixed.hboard");

        let png = make_test_png(4, 4);
        let items = vec![
            BoardItem::Image(ImageItem {
                texture: None,
                original_bytes: Arc::from(png.as_slice()),
                original_size: Vec2::new(4.0, 4.0),
                transform: Transform::default(),
                crop_rect: None,
                opacity: 1.0,
                grayscale: false,
                flip_h: false,
                flip_v: false,
                labels: Vec::new(),
                border_color: egui::Color32::TRANSPARENT,
            }),
            BoardItem::Text(TextItem {
                content: "note".into(),
                font_size: 16.0,
                color: egui::Color32::WHITE,
                bg_color: egui::Color32::TRANSPARENT,
                border_color: egui::Color32::TRANSPARENT,
                transform: Transform::default().with_position(Vec2::new(50.0, 50.0)),
            }),
        ];

        save_board(&path, &items).unwrap();

        let loaded = load_board(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        assert!(matches!(&loaded[0], BoardItem::Image(_)));
        assert!(matches!(&loaded[1], BoardItem::Text(_)));

        let _ = std::fs::remove_file(&path);
    }
}
