//! the app icon: its embedded source, and that the icon files the executable and the
//! window carry are exactly what the source makes.

use marshal_tray::{APP_ICON_SIZES, Bitmap, app_icon, ico, png};

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
