//! the app icon, which the executable and the window carry, in the same embedded text
//! form as the notification-area glyphs: a tile and a mark for each size, composed here.
//! The files `icons/icon.ico` and `icons/icon.png` are written from it by the `icons`
//! example, and a test keeps them equal to what it writes.

use crate::glyphs::{Bitmap, block};

/// the sizes the app icon is drawn at, in pixels; the largest is the one `icon.png` holds.
pub const APP_ICON_SIZES: [u32; 6] = [16, 20, 24, 32, 40, 48];

const SOURCE: &str = include_str!("../glyphs/app-icon.txt");

/// Radix Iris step 9, the tile's colour.
const TILE: [u32; 3] = [0x5b, 0x5b, 0xd6];
const MARK: [u32; 3] = [0xff, 0xff, 0xff];

/// the app icon `size` pixels square, which is blank for a size the source does not
/// hold; a test holds it to every one of [`APP_ICON_SIZES`].
#[must_use]
pub fn app_icon(size: u32) -> Bitmap {
    let tile = block(SOURCE, &format!("tile {size}"), size).unwrap_or_default();
    let mark = block(SOURCE, &format!("mark {size}"), size).unwrap_or_default();
    let mut rgba = Vec::with_capacity(tile.len() * 4);
    for (tile_alpha, mark_alpha) in tile.iter().zip(&mark) {
        let (under, over) = (u32::from(*tile_alpha), u32::from(*mark_alpha));
        // the mark over the tile, in straight alpha: what the tile shows through the
        // mark's partly clear pixels is weighted by how clear they are.
        let alpha = over * 255 + under * (255 - over);
        for (tile_part, mark_part) in TILE.iter().zip(MARK) {
            let part = (mark_part * over * 255 + tile_part * under * (255 - over) + alpha / 2)
                .checked_div(alpha)
                .unwrap_or(0);
            rgba.push(u8::try_from(part).unwrap_or(u8::MAX));
        }
        rgba.push(u8::try_from((alpha + 127) / 255).unwrap_or(u8::MAX));
    }
    Bitmap { size, rgba }
}
