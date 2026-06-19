//! Storage codec: converts between domain types and storage-shaped primitives.
//!
//! Persistence (SQLite) and other storage backends share these conversions so
//! the encoding lives in one canonical place, free of backend-specific types.

use egui::Color32;

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
}
