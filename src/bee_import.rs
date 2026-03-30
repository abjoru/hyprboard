use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use egui::Vec2;
use rusqlite::{Connection, params};

use crate::items::{BoardItem, ImageItem, ItemId, Transform, image_dimensions};

/// Import a .bee (BeeRef) file, returning board items.
/// BeeRef uses SQLite with `items` table + `sqlar` table for image blobs.
pub fn import_bee(path: &Path) -> Result<Vec<BoardItem>, String> {
    let conn = Connection::open(path).map_err(|e| format!("open: {e}"))?;

    // Check if sqlar table exists (BeeRef stores images there)
    let has_sqlar: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='sqlar'")
        .and_then(|mut s| s.query_row([], |_| Ok(true)))
        .unwrap_or(false);

    let mut stmt = conn
        .prepare(
            "SELECT id, type, x, y, z, scale, rotation, data
             FROM items ORDER BY z ASC",
        )
        .map_err(|e| format!("query items: {e}"))?;

    let mut items = Vec::new();
    let mut next_id: u64 = 0;

    let rows = stmt
        .query_map([], |row| {
            Ok(BeeRow {
                id: row.get(0)?,
                item_type: row.get(1)?,
                x: row.get(2)?,
                y: row.get(3)?,
                _z: row.get(4)?,
                scale: row.get(5)?,
                rotation: row.get(6)?,
                data: row.get(7)?,
            })
        })
        .map_err(|e| format!("read items: {e}"))?;

    for row in rows {
        let row = row.map_err(|e| format!("row: {e}"))?;

        // Parse JSON data field for extra properties
        let props: BeeProps = row
            .data
            .as_deref()
            .and_then(|d| serde_json::from_str(d).ok())
            .unwrap_or_default();

        match row.item_type.as_str() {
            "image" | "pixmap" => {
                // Try loading image from sqlar table
                let image_bytes = if has_sqlar {
                    load_sqlar_image(&conn, row.id)?
                } else {
                    None
                };

                let Some(bytes) = image_bytes else {
                    log::warn!("BeeRef item {} has no image data, skipping", row.id);
                    continue;
                };

                let original_size = image_dimensions(&bytes).unwrap_or(Vec2::new(100.0, 100.0));

                // BeeRef: uniform scale -> Vec2::splat
                let scale_val = row.scale.unwrap_or(1.0) as f32;
                // BeeRef: rotation in degrees (Qt convention) -> radians
                let rotation_deg = row.rotation.unwrap_or(0.0) as f32;
                let rotation_rad = rotation_deg.to_radians();

                // Check for flip in props (scale -1 means flip)
                let flip_h = props.flip.is_some_and(|f| f < 0.0);

                let crop_rect = props.crop.as_ref().and_then(|crop| {
                    if crop.len() == 4 {
                        Some(egui::Rect::from_min_size(
                            egui::pos2(crop[0] as f32, crop[1] as f32),
                            Vec2::new(crop[2] as f32, crop[3] as f32),
                        ))
                    } else {
                        None
                    }
                });

                next_id += 1;
                items.push(BoardItem::Image(ImageItem {
                    id: ItemId(next_id),
                    texture: None,
                    original_bytes: Arc::from(bytes),
                    original_size,
                    transform: Transform {
                        position: Vec2::new(row.x as f32, row.y as f32),
                        rotation: rotation_rad,
                        scale: Vec2::splat(scale_val.abs()),
                    },
                    crop_rect,
                    opacity: props.opacity.unwrap_or(1.0) as f32,
                    grayscale: props.grayscale.unwrap_or(false),
                    flip_h,
                    flip_v: false,
                    border_color: egui::Color32::TRANSPARENT,
                }));
            }
            "text" => {
                next_id += 1;
                let content = props.text.unwrap_or_default();
                items.push(BoardItem::new_text(
                    ItemId(next_id),
                    content,
                    Vec2::new(row.x as f32, row.y as f32),
                ));
            }
            other => {
                log::debug!("Skipping unknown BeeRef item type: {other}");
            }
        }
    }

    log::info!("Imported {} items from BeeRef file", items.len());
    Ok(items)
}

fn load_sqlar_image(conn: &Connection, item_id: i64) -> Result<Option<Vec<u8>>, String> {
    // BeeRef stores images in sqlar table, keyed by item id
    // The data column may be zlib-compressed if length(data) < sz
    let result: Option<(Vec<u8>, i64, i64)> = conn
        .prepare("SELECT data, sz, length(data) FROM sqlar WHERE name = ?1")
        .and_then(|mut s| {
            s.query_row(params![item_id.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
        })
        .ok();

    // Also try with numeric name matching
    let result = result.or_else(|| {
        conn.prepare("SELECT data, sz, length(data) FROM sqlar WHERE rowid = ?1")
            .and_then(|mut s| {
                s.query_row(params![item_id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
            })
            .ok()
    });

    let Some((data, sz, data_len)) = result else {
        return Ok(None);
    };

    // If data_len < sz, data is zlib-compressed
    if data_len < sz {
        let mut decoder = flate2::read::ZlibDecoder::new(&data[..]);
        let mut decompressed = Vec::with_capacity(sz as usize);
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| format!("zlib decompress: {e}"))?;
        Ok(Some(decompressed))
    } else {
        Ok(Some(data))
    }
}

struct BeeRow {
    id: i64,
    item_type: String,
    x: f64,
    y: f64,
    _z: f64,
    scale: Option<f64>,
    rotation: Option<f64>,
    data: Option<String>,
}

#[derive(Default, serde::Deserialize)]
struct BeeProps {
    #[serde(default)]
    opacity: Option<f64>,
    #[serde(default)]
    grayscale: Option<bool>,
    #[serde(default)]
    flip: Option<f64>,
    #[serde(default)]
    crop: Option<Vec<f64>>,
    #[serde(default)]
    text: Option<String>,
}
