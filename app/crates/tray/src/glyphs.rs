//! the notification-area icon's images. They are embedded as text, one mask of alpha
//! values for each state, colour and size, drawn by hand on that size's own pixel grid
//! (see the headers of `glyphs/white.txt` and `glyphs/near-black.txt`), so they can be
//! read in a review and the app links no image decoder. The selection by state, colour
//! and scale is in [`crate::watch`].

use marshal_view::{GlyphColour, IconState};

/// the sizes the notification area asks for, at 100, 125, 150, 175 and 200 percent
/// scaling, in pixels.
pub const SIZES: [u32; 5] = [16, 20, 24, 28, 32];

const WHITE: &str = include_str!("../glyphs/white.txt");
const NEAR_BLACK: &str = include_str!("../glyphs/near-black.txt");

/// an image as red, green, blue and alpha bytes, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub size: u32,
    pub rgba: Vec<u8>,
}

/// the name a state's blocks carry in the glyph files.
fn block_name(state: IconState) -> &'static str {
    match state {
        IconState::AllUp => "all-up",
        IconState::SomeNotUp => "some-not-up",
        IconState::Stopped => "stopped",
        IconState::StoppedByError => "stopped-by-error",
        IconState::NotInControl => "not-in-control",
    }
}

/// the file that holds the glyphs drawn for `colour`.
fn file(colour: GlyphColour) -> &'static str {
    match colour {
        GlyphColour::White => WHITE,
        GlyphColour::NearBlack => NEAR_BLACK,
    }
}

/// the alpha a mask character stands for: `.` is none and a hex digit is its value in
/// fifteenths of opaque, so `f` is opaque.
fn alpha_of(character: char) -> Option<u8> {
    match character {
        '.' => Some(0),
        digit => digit
            .to_digit(16)
            .and_then(|fifteenths| u8::try_from(fifteenths * 17).ok()),
    }
}

/// the alpha values of the block headed `header` in `text`, row by row, or `None` when
/// the block is missing or is not `size` rows of `size` characters.
pub(crate) fn block(text: &str, header: &str, size: u32) -> Option<Vec<u8>> {
    let side = usize::try_from(size).ok()?;
    let mut lines = text
        .lines()
        .skip_while(|line| line.trim() != header)
        .skip(1);
    let mut alpha = Vec::with_capacity(side * side);
    for _ in 0..side {
        let line = lines.next()?;
        if line.chars().count() != side {
            return None;
        }
        for character in line.chars() {
            alpha.push(alpha_of(character)?);
        }
    }
    Some(alpha)
}

/// the image for `state` in `colour`, `size` pixels square, which is blank for a state,
/// colour and size the glyph files do not hold; a test holds them to every one of
/// [`SIZES`].
#[must_use]
pub fn bitmap(state: IconState, colour: GlyphColour, size: u32) -> Bitmap {
    let marshal_view::Rgb(red, green, blue) = colour.rgb();
    let header = format!("{} {size}", block_name(state));
    let alpha = block(file(colour), &header, size).unwrap_or_default();
    let mut rgba = Vec::with_capacity(alpha.len() * 4);
    for value in alpha {
        rgba.extend_from_slice(&[red, green, blue, value]);
    }
    Bitmap { size, rgba }
}
