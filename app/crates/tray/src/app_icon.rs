//! the app icon, which the executable and the window carry, in the same embedded text
//! form as the notification-area glyphs: a tile and a mark for each size, composed here.
//! The files `icons/icon.ico` and `icons/icon.png` are written from it by the `icons`
//! example, and a test keeps them equal to what it writes.

use crate::glyphs::{Bitmap, block};

/// the sizes the app icon is drawn at, in pixels; the largest is the one `icon.png` holds.
pub const APP_ICON_SIZES: [u32; 6] = [16, 20, 24, 32, 40, 48];

/// the sizes the Mac's app icon is drawn at, in pixels, which `icon.icns` holds.
pub const MAC_ICON_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];

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

/// the side of W1's largest drawing, whose grid the Mac's icon scales.
const GRID: f64 = 48.0;
/// the mark's three lines on that grid: left, right and top; each is 4 units high with
/// round ends, as the 48 px drawing has them.
const LINES: [(f64, f64, f64); 3] = [(14.0, 34.0, 14.0), (14.0, 34.0, 22.0), (14.0, 29.0, 30.0)];
/// samples a pixel takes on each axis, for smooth edges at every size.
const SAMPLES: u32 = 4;

/// the Mac's app icon `size` pixels square: the same tile and mark, drawn on Apple's icon
/// grid, where the tile is 824 of 1024 units with a corner of 22.5% of its side, so it
/// sits beside other apps' icons in the Dock and Finder.
#[must_use]
pub fn mac_app_icon(size: u32) -> Bitmap {
    let side = f64::from(size);
    let body = side * 824.0 / 1024.0;
    let margin = (side - body) / 2.0;
    let radius = body * 0.225;
    let unit = body / GRID;
    let step = 1.0 / f64::from(SAMPLES);
    let count = SAMPLES * SAMPLES;
    let mut rgba = Vec::with_capacity(usize::try_from(size * size * 4).unwrap_or(0));
    for row in 0..size {
        for column in 0..size {
            let (mut tile, mut mark) = (0_u32, 0_u32);
            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let x = f64::from(column) + (f64::from(sample_x) + 0.5) * step;
                    let y = f64::from(row) + (f64::from(sample_y) + 0.5) * step;
                    if in_rounded_square(x - margin, y - margin, body, radius) {
                        tile += 1;
                        let (grid_x, grid_y) = ((x - margin) / unit, (y - margin) / unit);
                        if LINES.iter().any(|line| in_line(grid_x, grid_y, *line)) {
                            mark += 1;
                        }
                    }
                }
            }
            // the mark is white over the tile, so a pixel is the tile's colour mixed
            // with white by the mark's share of the samples the tile covers, at the
            // tile's coverage.
            for (tile_part, mark_part) in TILE.iter().zip(MARK) {
                let part = (tile_part * (tile - mark) + mark_part * mark + tile / 2)
                    .checked_div(tile)
                    .unwrap_or(0);
                rgba.push(u8::try_from(part).unwrap_or(u8::MAX));
            }
            rgba.push(u8::try_from((255 * tile + count / 2) / count).unwrap_or(u8::MAX));
        }
    }
    Bitmap { size, rgba }
}

/// whether (x, y) lies in a square of side `body` at the origin with corners of `radius`.
fn in_rounded_square(x: f64, y: f64, body: f64, radius: f64) -> bool {
    if !(0.0..body).contains(&x) || !(0.0..body).contains(&y) {
        return false;
    }
    let nearest_x = x.clamp(radius, body - radius);
    let nearest_y = y.clamp(radius, body - radius);
    (x - nearest_x).powi(2) + (y - nearest_y).powi(2) <= radius * radius
}

/// whether a point on the grid lies in a line from `left` to `right` that starts at
/// `top`: a bar 4 units high with half-round ends.
fn in_line(x: f64, y: f64, (left, right, top): (f64, f64, f64)) -> bool {
    let half = 2.0;
    let centre_y = top + half;
    let nearest_x = x.clamp(left + half, right - half);
    (x - nearest_x).powi(2) + (y - centre_y).powi(2) <= half * half
}
