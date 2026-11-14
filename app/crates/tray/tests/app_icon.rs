//! the app icon: its embedded source, and that the icon files the executable and the
//! window carry are exactly what the source makes.

use marshal_tray::{
    APP_ICON_SIZES, Bitmap, MAC_ICON_SIZES, app_icon, icns, ico, mac_app_icon, png,
};

#[test]
fn every_size_is_a_tile_with_the_mark_on_it() {
    for size in APP_ICON_SIZES {
        let image = app_icon(size);
        let side = size as usize;
        assert_eq!(image.size, size);
        assert_eq!(image.rgba.len(), side * side * 4);
        let pixels: Vec<&[u8]> = image.rgba.chunks(4).collect();
        let centre = pixels[side * (side / 2) + side / 2];
        assert_eq!(centre[3], 255, "{size}: the middle of the tile is solid");
        assert!(
            pixels
                .iter()
                .any(|pixel| pixel[3] == 255 && pixel[..3] == [0x5b, 0x5b, 0xd6]),
            "{size}: has tile pixels"
        );
        assert!(
            pixels
                .iter()
                .any(|pixel| pixel[3] == 255 && pixel[..3] == [255, 255, 255]),
            "{size}: has mark pixels"
        );
        // the tile's corners are rounded, so the corner pixel is clear.
        assert_eq!(pixels[0][3], 0, "{size}: the corner is clear");
    }
}

#[test]
fn the_icon_files_are_what_the_source_makes() {
    let images: Vec<Bitmap> = APP_ICON_SIZES.iter().map(|size| app_icon(*size)).collect();
    assert_eq!(
        ico(&images),
        include_bytes!("../../app/icons/icon.ico"),
        "icon.ico differs; run `cargo run --example icons -- app-icon crates/app/icons` in app/"
    );
    let largest = images.last().expect("a size");
    assert_eq!(
        png(largest),
        include_bytes!("../../app/icons/icon.png"),
        "icon.png differs; run `cargo run --example icons -- app-icon crates/app/icons` in app/"
    );
}

#[test]
fn the_macs_icon_is_the_tile_on_apples_grid_with_a_clear_margin() {
    for size in [16_u32, 1024] {
        let image = mac_app_icon(size);
        let side = size as usize;
        let pixels: Vec<&[u8]> = image.rgba.chunks(4).collect();
        let at = |x: usize, y: usize| pixels[y * side + x];
        // the tile is 824 of 1024 units, so the outer 100 units are clear on each side
        let margin = side * 100 / 1024 - 1;
        assert_eq!(at(margin, side / 2)[3], 0, "{size}: the margin is clear");
        assert_eq!(at(side / 2, margin)[3], 0, "{size}: the margin is clear");
        assert_eq!(
            at(side * 11 / 100, side * 11 / 100)[3],
            0,
            "{size}: the corner is round"
        );
        assert_eq!(
            at(side * 3 / 4, side * 3 / 4),
            [0x5b, 0x5b, 0xd6, 255],
            "{size}: tile"
        );
    }
    // the first line runs from 14 to 34 of W1's 48 units, 14 units down the tile
    let image = mac_app_icon(1024);
    let unit = 824.0 / 48.0;
    let point = |grid: f64| (100.0 + grid * unit) as usize;
    let pixel = |x: usize, y: usize| &image.rgba[(y * 1024 + x) * 4..][..4];
    assert_eq!(
        pixel(point(24.0), point(16.0)),
        [255, 255, 255, 255],
        "the first line"
    );
    assert_eq!(
        pixel(point(31.5), point(32.0)),
        [0x5b, 0x5b, 0xd6, 255],
        "the third is short"
    );
    assert_eq!(
        pixel(point(24.0), point(20.0)),
        [0x5b, 0x5b, 0xd6, 255],
        "between lines"
    );
}

#[test]
fn the_macs_icon_file_is_what_the_source_makes() {
    let images: Vec<Bitmap> = MAC_ICON_SIZES
        .iter()
        .map(|size| mac_app_icon(*size))
        .collect();
    assert!(
        icns(&images) == include_bytes!("../../app/icons/icon.icns"),
        "icon.icns differs; run `cargo run --example icons -- mac-icon crates/app/icons` in app/"
    );
}
