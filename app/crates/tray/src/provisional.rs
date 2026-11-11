//! the notification-area icon's images, provisional. They are drawn here, as the decoded
//! pixels the notification area takes, so the app links no image decoder; the final
//! icon sheet replaces this file's [`bitmap`] with images embedded already decoded, for
//! each state in both colours at each of [`SIZES`], and nothing else changes: the
//! selection by state, colour and scale is in [`crate::watch`].

use marshal_view::{GlyphColour, IconState};

/// the sizes the notification area asks for, at 100, 125, 150, 175 and 200 percent
/// scaling, in pixels.
pub const SIZES: [u32; 5] = [16, 20, 24, 28, 32];

/// an image as red, green, blue and alpha bytes, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub size: u32,
    pub rgba: Vec<u8>,
}

/// how many sample points a pixel's coverage is measured with, on each axis.
const SAMPLES: u32 = 4;

/// the ring every state has an outline of, as fractions of the image's half width.
const RING_INNER: f64 = 0.62;
const RING_OUTER: f64 = 0.94;

/// whether the point (`x`, `y`), each from -1 to 1 with y down, is inside the glyph of
/// `state`.
fn inside(state: IconState, x: f64, y: f64) -> bool {
    let radius = x.hypot(y);
    let ring = (RING_INNER..=RING_OUTER).contains(&radius);
    match state {
        IconState::AllUp => radius <= RING_OUTER,
        // half the disc is full, as half of the runners are up
        IconState::SomeNotUp => ring || (x < 0.0 && radius < RING_INNER),
        IconState::Stopped => ring || (x.abs() <= 0.26 && y.abs() <= 0.26),
        IconState::StoppedByError => {
            let bar = x.abs() <= 0.12 && (-0.5..=0.14).contains(&y);
            let dot = x.abs() <= 0.14 && (0.3..=0.52).contains(&y);
            ring || bar || dot
        }
        IconState::NotInControl => {
            // a ring in eight pieces
            let turn = y.atan2(x) + std::f64::consts::PI;
            let piece = (turn / (std::f64::consts::TAU / 8.0)).floor();
            ring && piece % 2.0 == 0.0
        }
    }
}

/// the image for `state` in `colour`, `size` pixels square.
#[must_use]
pub fn bitmap(state: IconState, colour: GlyphColour, size: u32) -> Bitmap {
    let marshal_view::Rgb(red, green, blue) = colour.rgb();
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let across = f64::from(size * SAMPLES);
    for row in 0..size {
        for column in 0..size {
            let mut covered = 0;
            for sample_row in 0..SAMPLES {
                for sample_column in 0..SAMPLES {
                    let x =
                        (f64::from(column * SAMPLES + sample_column) + 0.5) / across * 2.0 - 1.0;
                    let y = (f64::from(row * SAMPLES + sample_row) + 0.5) / across * 2.0 - 1.0;
                    if inside(state, x, y) {
                        covered += 1;
                    }
                }
            }
            let alpha = covered * 255 / (SAMPLES * SAMPLES);
            rgba.extend_from_slice(&[red, green, blue, u8::try_from(alpha).unwrap_or(u8::MAX)]);
        }
    }
    Bitmap { size, rgba }
}
