use std::path::Path;
use std::sync::Arc;

use egui::{Color32, Vec2};
use rusqlite::{Connection, params};

use crate::codec::{ImageRecord, ItemRecord, TextRecord, color_to_u32, u32_to_color};
use crate::items::{BoardItem, Connector, ConnectorId, ItemId, TextItem, Transform};

const SCHEMA_VERSION: &str = "4";

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
            border_color INTEGER,
            item_id    INTEGER
        );
        CREATE TABLE IF NOT EXISTS connectors (
            id         INTEGER PRIMARY KEY,
            from_id    INTEGER NOT NULL,
            to_id      INTEGER NOT NULL,
            color      INTEGER NOT NULL DEFAULT 3014898687,
            thickness  REAL NOT NULL DEFAULT 2.0
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

    let has_item_id: bool = conn.prepare("SELECT item_id FROM items LIMIT 0").is_ok();
    if !has_item_id {
        conn.execute("ALTER TABLE items ADD COLUMN item_id INTEGER", [])?;
        conn.execute("UPDATE items SET item_id = id WHERE item_id IS NULL", [])?;
        log::info!("Migrated schema: added item_id column");
    }

    let has_connectors: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='connectors'")
        .and_then(|mut s| s.query_row([], |_| Ok(true)))
        .unwrap_or(false);
    if !has_connectors {
        conn.execute_batch(
            "CREATE TABLE connectors (
                id         INTEGER PRIMARY KEY,
                from_id    INTEGER NOT NULL,
                to_id      INTEGER NOT NULL,
                color      INTEGER NOT NULL DEFAULT 3014898687,
                thickness  REAL NOT NULL DEFAULT 2.0
            );",
        )?;
        log::info!("Migrated schema: added connectors table");
    }

    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        params!["version", SCHEMA_VERSION],
    )?;

    Ok(())
}

pub fn save_board(
    path: &Path,
    items: &[BoardItem],
    connectors: &[Connector],
) -> rusqlite::Result<()> {
    // Write to temp file then rename for atomicity
    let tmp_path = path.with_extension("hboard.tmp");
    {
        let conn = Connection::open(&tmp_path)?;
        conn.execute_batch("PRAGMA journal_mode=OFF;")?;
        create_schema(&conn)?;

        conn.execute("DELETE FROM connectors", [])?;
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
                bg_color, border_color, item_id
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16,
                ?17, ?18, ?19,
                ?20, ?21,
                ?22, ?23, ?24
            )",
        )?;

        for (z_order, item) in items.iter().enumerate() {
            // z_order tracks list position; the rest is the storage-shaped
            // record, binding each variant's live columns and NULL for the rest.
            match ItemRecord::from_item(item) {
                ItemRecord::Image(r) => {
                    let (cx, cy, cw, ch) = r
                        .crop
                        .map(|(x, y, w, h)| {
                            (
                                Some(x as f64),
                                Some(y as f64),
                                Some(w as f64),
                                Some(h as f64),
                            )
                        })
                        .unwrap_or((None, None, None, None));
                    let (iw, ih) = r
                        .dimensions
                        .map(|(w, h)| (Some(w as f64), Some(h as f64)))
                        .unwrap_or((None, None));

                    stmt.execute(params![
                        z_order as i64,
                        "image",
                        r.pos.0 as f64,
                        r.pos.1 as f64,
                        r.scale.0 as f64,
                        r.scale.1 as f64,
                        r.rotation as f64,
                        &*r.bytes,
                        cx,
                        cy,
                        cw,
                        ch,
                        r.opacity as f64,
                        r.grayscale as i32,
                        r.flip_h as i32,
                        r.flip_v as i32,
                        None::<String>,
                        None::<f64>,
                        None::<i64>,
                        iw,
                        ih,
                        None::<i64>,
                        r.border_color as i64,
                        r.id as i64,
                    ])?;
                }
                ItemRecord::Text(r) => {
                    stmt.execute(params![
                        z_order as i64,
                        "text",
                        r.pos.0 as f64,
                        r.pos.1 as f64,
                        r.scale.0 as f64,
                        r.scale.1 as f64,
                        r.rotation as f64,
                        None::<Vec<u8>>,
                        None::<f64>,
                        None::<f64>,
                        None::<f64>,
                        None::<f64>,
                        None::<f64>,
                        None::<i32>,
                        None::<i32>,
                        None::<i32>,
                        &r.content,
                        r.font_size as f64,
                        r.color,
                        None::<f64>,
                        None::<f64>,
                        r.bg_color as i64,
                        r.border_color as i64,
                        r.id as i64,
                    ])?;
                }
            }
        }

        // Save connectors
        let mut conn_stmt = conn.prepare(
            "INSERT INTO connectors (id, from_id, to_id, color, thickness)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for c in connectors {
            conn_stmt.execute(params![
                c.id.0 as i64,
                c.from.0 as i64,
                c.to.0 as i64,
                color_to_u32(c.color) as i64,
                c.thickness as f64,
            ])?;
        }
    }

    std::fs::rename(&tmp_path, path)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    Ok(())
}

/// Returns (items, connectors, max_id) so Board can resume ID allocation.
pub fn load_board(path: &Path) -> rusqlite::Result<(Vec<BoardItem>, Vec<Connector>, u64)> {
    let conn = Connection::open(path)?;

    // Migrate if needed
    migrate_schema(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT id, item_type, pos_x, pos_y, scale_x, scale_y, rotation,
                image_data, crop_x, crop_y, crop_w, crop_h,
                opacity, grayscale, flip_h, flip_v,
                content, font_size, color,
                img_width, img_height,
                bg_color, border_color, item_id
         FROM items ORDER BY z_order ASC",
    )?;

    // Check if legacy labels table exists for migration
    let has_labels_table: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='labels'")
        .and_then(|mut s| s.query_row([], |_| Ok(true)))
        .unwrap_or(false);

    let mut label_stmt = if has_labels_table {
        Some(conn.prepare(
            "SELECT text, offset_x, offset_y, font_size, color
             FROM labels WHERE item_id = ?1",
        )?)
    } else {
        None
    };

    let mut items = Vec::new();
    let mut max_id: u64 = 0;
    let mut fallback_id: u64 = 0;

    // Read each row's columns into a storage record (id placeholder 0, resolved
    // below); the db primary key and raw item_id ride alongside for id
    // allocation and the legacy-label join.
    let rows = stmt.query_map([], |row| {
        let db_id: i64 = row.get(0)?;
        let item_type: String = row.get(1)?;
        let item_id: Option<i64> = row.get(23)?;
        let pos = (row.get::<_, f64>(2)? as f32, row.get::<_, f64>(3)? as f32);
        let scale = (row.get::<_, f64>(4)? as f32, row.get::<_, f64>(5)? as f32);
        let rotation = row.get::<_, f64>(6)? as f32;

        let record = match item_type.as_str() {
            "image" => match row.get::<_, Option<Vec<u8>>>(7)? {
                Some(bytes) => {
                    let crop = match (
                        row.get::<_, Option<f64>>(8)?,
                        row.get::<_, Option<f64>>(9)?,
                        row.get::<_, Option<f64>>(10)?,
                        row.get::<_, Option<f64>>(11)?,
                    ) {
                        (Some(x), Some(y), Some(w), Some(h)) => {
                            Some((x as f32, y as f32, w as f32, h as f32))
                        }
                        _ => None,
                    };
                    let dimensions = match (
                        row.get::<_, Option<f64>>(19)?,
                        row.get::<_, Option<f64>>(20)?,
                    ) {
                        (Some(w), Some(h)) => Some((w as f32, h as f32)),
                        _ => None,
                    };
                    Some(ItemRecord::Image(ImageRecord {
                        id: 0,
                        pos,
                        scale,
                        rotation,
                        bytes: Arc::from(bytes),
                        dimensions,
                        crop,
                        opacity: row.get::<_, Option<f64>>(12)?.unwrap_or(1.0) as f32,
                        grayscale: row.get::<_, Option<i32>>(13)?.unwrap_or(0) != 0,
                        flip_h: row.get::<_, Option<i32>>(14)?.unwrap_or(0) != 0,
                        flip_v: row.get::<_, Option<i32>>(15)?.unwrap_or(0) != 0,
                        border_color: row.get::<_, Option<i64>>(22)?.unwrap_or(0) as u32,
                    }))
                }
                None => None,
            },
            "text" => Some(ItemRecord::Text(TextRecord {
                id: 0,
                pos,
                scale,
                rotation,
                content: row.get::<_, Option<String>>(16)?.unwrap_or_default(),
                font_size: row.get::<_, Option<f64>>(17)?.unwrap_or(16.0) as f32,
                color: row.get::<_, Option<i64>>(18)?.unwrap_or(0xFFFFFFFF) as u32,
                bg_color: row.get::<_, Option<i64>>(21)?.unwrap_or(0) as u32,
                border_color: row.get::<_, Option<i64>>(22)?.unwrap_or(0) as u32,
            })),
            _ => None,
        };

        Ok((db_id, item_id, record))
    })?;

    for row in rows {
        let (db_id, item_id, record) = row?;

        // Resolve the stable id for every row (fallback counter advances even
        // for skipped rows, matching prior load ordering).
        let id = match item_id {
            Some(v) => {
                let v = v as u64;
                max_id = max_id.max(v);
                v
            }
            None => {
                fallback_id += 1;
                fallback_id
            }
        };

        let Some(mut record) = record else { continue };
        let image_pos = match &mut record {
            ItemRecord::Image(r) => {
                r.id = id;
                Some(Vec2::new(r.pos.0, r.pos.1))
            }
            ItemRecord::Text(r) => {
                r.id = id;
                None
            }
        };

        items.push(record.into_item());

        // Migrate legacy labels to standalone TextItems (images only)
        if let (Some(image_pos), Some(ref mut lstmt)) = (image_pos, label_stmt.as_mut()) {
            let legacy_labels: Vec<(String, f64, f64, f64, i64)> = lstmt
                .query_map(params![db_id], |lrow| {
                    Ok((
                        lrow.get(0)?,
                        lrow.get(1)?,
                        lrow.get(2)?,
                        lrow.get(3)?,
                        lrow.get(4)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            for (text, ox, oy, fs, cv) in legacy_labels {
                fallback_id += 1;
                let text_id = ItemId(fallback_id);
                let abs_pos = Vec2::new(image_pos.x + ox as f32, image_pos.y + oy as f32);
                items.push(BoardItem::Text(TextItem {
                    id: text_id,
                    content: text,
                    font_size: fs as f32,
                    color: u32_to_color(cv as u32),
                    bg_color: Color32::from_rgba_premultiplied(0, 0, 0, 180),
                    border_color: Color32::TRANSPARENT,
                    transform: Transform::default().with_position(abs_pos),
                    cached_size: Vec2::ZERO,
                }));
            }
        }
    }

    max_id = max_id.max(fallback_id);

    // Load connectors
    let mut loaded_connectors = Vec::new();
    let has_connectors_table: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='connectors'")
        .and_then(|mut s| s.query_row([], |_| Ok(true)))
        .unwrap_or(false);

    if has_connectors_table {
        let mut cstmt =
            conn.prepare("SELECT id, from_id, to_id, color, thickness FROM connectors")?;
        let crows = cstmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })?;
        for crow in crows {
            let (cid, from_id, to_id, color, thickness) = crow?;
            let cid = cid as u64;
            max_id = max_id.max(cid);
            loaded_connectors.push(Connector {
                id: ConnectorId(cid),
                from: ItemId(from_id as u64),
                to: ItemId(to_id as u64),
                color: u32_to_color(color as u32),
                thickness: thickness as f32,
            });
        }
    }

    Ok((items, loaded_connectors, max_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::ImageItem;
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
            id: ItemId(1),
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
            border_color: egui::Color32::TRANSPARENT,
        })];

        save_board(&path, &items, &[]).unwrap();

        let (loaded, _, max_id) = load_board(&path).unwrap();
        assert_eq!(max_id, 1);

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
        assert_eq!(&img.original_bytes[..], &png[..]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn round_trip_text_item() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_roundtrip_text.hboard");

        let items = vec![BoardItem::Text(TextItem {
            id: ItemId(1),
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
            cached_size: Vec2::ZERO,
        })];

        save_board(&path, &items, &[]).unwrap();

        let (loaded, _, _) = load_board(&path).unwrap();

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
                id: ItemId(1),
                texture: None,
                original_bytes: Arc::from(png.as_slice()),
                original_size: Vec2::new(4.0, 4.0),
                transform: Transform::default(),
                crop_rect: None,
                opacity: 1.0,
                grayscale: false,
                flip_h: false,
                flip_v: false,
                border_color: egui::Color32::TRANSPARENT,
            }),
            BoardItem::Text(TextItem {
                id: ItemId(2),
                content: "note".into(),
                font_size: 16.0,
                color: egui::Color32::WHITE,
                bg_color: egui::Color32::TRANSPARENT,
                border_color: egui::Color32::TRANSPARENT,
                transform: Transform::default().with_position(Vec2::new(50.0, 50.0)),
                cached_size: Vec2::ZERO,
            }),
        ];

        save_board(&path, &items, &[]).unwrap();

        let (loaded, _, _) = load_board(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        assert!(matches!(&loaded[0], BoardItem::Image(_)));
        assert!(matches!(&loaded[1], BoardItem::Text(_)));

        let _ = std::fs::remove_file(&path);
    }
}
