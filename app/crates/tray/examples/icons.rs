//! writes the app icon's files and the icon sheet from the embedded images.
//!
//! `cargo run --example icons -- app-icon <folder>` writes `icon.ico` and `icon.png` into
//! `<folder>`, `cargo run --example icons -- mac-icon <folder>` writes the Mac's `icon.icns`,
//! and `cargo run --example icons -- sheet <file.png>` writes the sheet the
//! maintainer approves the icons from: every notification-area state at each size in
//! both colours on a light and a dark taskbar, and the app icon at its sizes, as the
//! pixels the app embeds, doubled in size without smoothing.

use std::path::Path;
use std::process::ExitCode;

use marshal_tray::{
    APP_ICON_SIZES, Bitmap, MAC_ICON_SIZES, SIZES, app_icon, bitmap, icns, ico, mac_app_icon, png,
    png_rgba,
};
use marshal_view::{GlyphColour, IconState};

/// the taskbar backgrounds the design record draws the glyphs on.
const LIGHT: [u8; 3] = [0xee, 0xee, 0xee];
const DARK: [u8; 3] = [0x1f, 0x1f, 0x1f];
const PAGE: [u8; 3] = [0xff, 0xff, 0xff];
const INK: [u8; 3] = [0x1a, 0x1a, 0x1a];

const MARGIN: u32 = 12;
const LABELS: u32 = 150;
const CELL: u32 = 44;
const PANEL: u32 = CELL * 5;
const GAP: u32 = 8;
const SCALE: u32 = 2;

/// the five states, in the order the sheet lists them, with what a row is called.
const STATES: [(IconState, &str); 5] = [
    (IconState::AllUp, "all runners up"),
    (IconState::SomeNotUp, "some not up"),
    (IconState::Stopped, "stopped by you"),
    (IconState::StoppedByError, "stopped by an error"),
    (IconState::NotInControl, "not in control"),
];

/// a panel of the sheet: the taskbar it is drawn on and the glyph colour it shows. The
/// first two are the pairs the app uses; the other two show a glyph on the wrong
/// taskbar: white on a light one is what an unreadable mode would draw there, and
/// near-black on the dark one, which is its own colour, cannot be seen at all.
struct Panel {
    title: &'static str,
    background: [u8; 3],
    colour: GlyphColour,
}

const PANELS: [Panel; 4] = [
    Panel {
        title: "light taskbar, near-black glyph",
        background: LIGHT,
        colour: GlyphColour::NearBlack,
    },
    Panel {
        title: "dark taskbar, white glyph",
        background: DARK,
        colour: GlyphColour::White,
    },
    Panel {
        title: "light, white: mode unreadable",
        background: LIGHT,
        colour: GlyphColour::White,
    },
    Panel {
        title: "dark, near-black: invisible",
        background: DARK,
        colour: GlyphColour::NearBlack,
    },
];

/// the 5 by 7 letters the sheet's labels are set in, each row a string of five pixels.
#[rustfmt::skip]
const FONT: &[(char, [&str; 7])] = &[
    ('A', [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"]),
    ('B', ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."]),
    ('C', [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."]),
    ('D', ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."]),
    ('E', ["#####", "#....", "#....", "####.", "#....", "#....", "#####"]),
    ('F', ["#####", "#....", "#....", "####.", "#....", "#....", "#...."]),
    ('G', [".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####"]),
    ('H', ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"]),
    ('I', [".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."]),
    ('J', ["..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."]),
    ('K', ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"]),
    ('L', ["#....", "#....", "#....", "#....", "#....", "#....", "#####"]),
    ('M', ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"]),
    ('N', ["#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#"]),
    ('O', [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."]),
    ('P', ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."]),
    ('Q', [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"]),
    ('R', ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"]),
    ('S', [".####", "#....", "#....", ".###.", "....#", "....#", "####."]),
    ('T', ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."]),
    ('U', ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."]),
    ('V', ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."]),
    ('W', ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"]),
    ('X', ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"]),
    ('Y', ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."]),
    ('Z', ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"]),
    ('0', [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."]),
    ('1', ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."]),
    ('2', [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"]),
    ('3', ["####.", "....#", "....#", ".###.", "....#", "....#", "####."]),
    ('4', ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."]),
    ('5', ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."]),
    ('6', ["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."]),
    ('7', ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."]),
    ('8', [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."]),
    ('9', [".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."]),
    ('-', [".....", ".....", ".....", "#####", ".....", ".....", "....."]),
    (',', [".....", ".....", ".....", ".....", "..#..", "..#..", ".#..."]),
    ('.', [".....", ".....", ".....", ".....", ".....", ".##..", ".##.."]),
    (':', [".....", ".##..", ".##..", ".....", ".##..", ".##..", "....."]),
];
const LETTER_PITCH: u32 = 6;

struct Canvas {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32, background: [u8; 3]) -> Self {
        let mut rgba = Vec::new();
        for _ in 0..width * height {
            rgba.extend_from_slice(&[background[0], background[1], background[2], 255]);
        }
        Self {
            width,
            height,
            rgba,
        }
    }

    fn pixel(&mut self, x: u32, y: u32) -> Option<&mut [u8]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let start = usize::try_from(y * self.width + x).ok()? * 4;
        self.rgba.get_mut(start..start + 4)
    }

    fn rectangle(&mut self, x: u32, y: u32, width: u32, height: u32, colour: [u8; 3]) {
        for row in y..y + height {
            for column in x..x + width {
                if let Some(pixel) = self.pixel(column, row) {
                    pixel.copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
                }
            }
        }
    }

    /// `image` with its top left corner at (`x`, `y`), blended over what is there by
    /// its alpha.
    fn blend(&mut self, image: &Bitmap, x: u32, y: u32) {
        let side = image.size;
        for (index, source) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
            let index = u32::try_from(index).unwrap_or(0);
            let (column, row) = (index % side, index / side);
            let alpha = u32::from(source[3]);
            if let Some(pixel) = self.pixel(x + column, y + row) {
                for (under, over) in pixel.iter_mut().zip(source).take(3) {
                    let mixed = u32::from(*under) * (255 - alpha) + u32::from(*over) * alpha;
                    *under = u8::try_from((mixed + 127) / 255).unwrap_or(u8::MAX);
                }
            }
        }
    }

    fn text(&mut self, words: &str, x: u32, y: u32, colour: [u8; 3]) {
        let mut at = x;
        for letter in words.chars().map(|letter| letter.to_ascii_uppercase()) {
            let rows = FONT
                .iter()
                .find(|(known, _)| *known == letter)
                .map(|(_, rows)| rows);
            for (row, pixels) in (0_u32..).zip(rows.into_iter().flatten()) {
                for (column, mark) in (0_u32..).zip(pixels.chars()) {
                    if mark == '#' {
                        self.rectangle(at + column, y + row, 1, 1, colour);
                    }
                }
            }
            at += LETTER_PITCH;
        }
    }

    /// every pixel as a `SCALE` by `SCALE` block, without smoothing.
    fn scaled(&self) -> Self {
        let mut larger = Self::new(self.width * SCALE, self.height * SCALE, [0, 0, 0]);
        for y in 0..larger.height {
            for x in 0..larger.width {
                let start = usize::try_from((y / SCALE) * self.width + x / SCALE).unwrap_or(0) * 4;
                if let (Some(from), Some(to)) =
                    (self.rgba.get(start..start + 4), larger.pixel(x, y))
                {
                    to.copy_from_slice(from);
                }
            }
        }
        larger
    }
}

fn sheet() -> Canvas {
    let width = MARGIN * 2 + LABELS + 4 * PANEL + 3 * GAP;
    let first_row = 64;
    let notification_area_end = first_row + 5 * CELL;
    let app_icon_top = notification_area_end + 20;
    let height = app_icon_top + 36 + 64 + MARGIN;
    let mut canvas = Canvas::new(width, height, PAGE);

    canvas.text(
        "Marshal icon sheet: the embedded images, doubled",
        MARGIN,
        MARGIN,
        INK,
    );
    canvas.text("Notification area", MARGIN, 49, INK);
    for (column, panel) in (0_u32..).zip(&PANELS) {
        let left = MARGIN + LABELS + column * (PANEL + GAP);
        canvas.text(panel.title, left, 30, INK);
        canvas.rectangle(left, 44, PANEL, 1 + 5 * CELL + 14, panel.background);
        for (index, size) in (0_u32..).zip(SIZES) {
            let label = format!("{size}");
            let label_colour = if panel.background == DARK {
                [0xdd; 3]
            } else {
                INK
            };
            canvas.text(&label, left + index * CELL + CELL / 2 - 6, 49, label_colour);
        }
        for (row, (state, _)) in (0_u32..).zip(STATES) {
            for (index, size) in (0_u32..).zip(SIZES) {
                let image = bitmap(state, panel.colour, size);
                let x = left + index * CELL + (CELL - size) / 2;
                let y = first_row + row * CELL + (CELL - size) / 2;
                canvas.blend(&image, x, y);
            }
        }
    }
    for (row, (_, name)) in (0_u32..).zip(STATES) {
        canvas.text(name, MARGIN, first_row + row * CELL + CELL / 2 - 3, INK);
    }

    canvas.text("App icon, 16 to 48 px", MARGIN, app_icon_top, INK);
    for (column, background) in (0_u32..).zip([LIGHT, DARK]) {
        let left = MARGIN + LABELS + column * (PANEL + GAP + 100);
        canvas.rectangle(left, app_icon_top + 18, PANEL + 100, 64, background);
        let mut x = left + 12;
        for size in APP_ICON_SIZES {
            canvas.blend(&app_icon(size), x, app_icon_top + 18 + (64 - size) / 2);
            x += size + 18;
        }
    }
    canvas
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|why| format!("{}: {why}", path.display()))
}

fn run(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [command, folder] if command == "app-icon" => {
            let folder = Path::new(folder);
            let images: Vec<Bitmap> = APP_ICON_SIZES.iter().map(|size| app_icon(*size)).collect();
            write(&folder.join("icon.ico"), &ico(&images))?;
            let largest = images.last().ok_or("the app icon has no sizes")?;
            write(&folder.join("icon.png"), &png(largest))
        }
        [command, folder] if command == "mac-icon" => {
            let images: Vec<Bitmap> = MAC_ICON_SIZES
                .iter()
                .map(|size| mac_app_icon(*size))
                .collect();
            write(&Path::new(folder).join("icon.icns"), &icns(&images))
        }
        [command, file] if command == "sheet" => {
            let canvas = sheet().scaled();
            write(
                Path::new(file),
                &png_rgba(canvas.width, canvas.height, &canvas.rgba),
            )
        }
        _ => Err(
            "usage: icons app-icon <folder> | icons mac-icon <folder> | icons sheet <file.png>"
                .to_string(),
        ),
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("error: {why}");
            ExitCode::FAILURE
        }
    }
}
