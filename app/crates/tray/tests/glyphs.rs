//! the notification-area icon's embedded images.

use std::collections::BTreeSet;

use marshal_tray::{SIZES, bitmap};
use marshal_view::{GlyphColour, IconState};

const STATES: [IconState; 5] = [
    IconState::AllUp,
    IconState::SomeNotUp,
    IconState::Stopped,
    IconState::StoppedByError,
    IconState::NotInControl,
];
const COLOURS: [GlyphColour; 2] = [GlyphColour::White, GlyphColour::NearBlack];

fn alpha(state: IconState, colour: GlyphColour, size: u32) -> Vec<u8> {
    bitmap(state, colour, size)
        .rgba
        .chunks(4)
        .filter_map(|pixel| pixel.last().copied())
        .collect()
}

#[test]
fn every_image_is_drawn_in_its_colour_and_leaves_the_edge_clear() {
    for state in STATES {
        for colour in COLOURS {
            let marshal_view::Rgb(red, green, blue) = colour.rgb();
            for size in SIZES {
                let side = size as usize;
                let image = bitmap(state, colour, size);
                assert_eq!(image.size, size);
                assert_eq!(image.rgba.len(), side * side * 4);
                let mut drawn = 0;
                for (index, pixel) in image.rgba.chunks(4).enumerate() {
                    assert_eq!(pixel[..3], [red, green, blue]);
                    let (row, column) = (index / side, index % side);
                    if row == 0 || column == 0 || row == side - 1 || column == side - 1 {
                        assert_eq!(pixel[3], 0, "{state:?} {colour:?} {size}");
                    }
                    if pixel[3] == 255 {
                        drawn += 1;
                    }
                }
                assert!(drawn >= side, "{state:?} {colour:?} {size} is mostly blank");
            }
        }
    }
}

#[test]
fn each_file_holds_one_block_for_every_state_and_size() {
    let expected: BTreeSet<String> = [
        "all-up",
        "some-not-up",
        "stopped",
        "stopped-by-error",
        "not-in-control",
    ]
    .iter()
    .flat_map(|name| SIZES.iter().map(move |size| format!("{name} {size}")))
    .collect();
    for text in [
        include_str!("../glyphs/white.txt"),
        include_str!("../glyphs/near-black.txt"),
    ] {
        let headers: Vec<&str> = text
            .lines()
            .filter(|line| line.contains(' ') && !line.starts_with('#'))
            .collect();
        assert_eq!(headers.len(), expected.len());
        assert_eq!(
            headers
                .iter()
                .map(|line| (*line).to_string())
                .collect::<BTreeSet<_>>(),
            expected
        );
    }
}

#[test]
fn the_five_states_differ_by_shape_at_every_size_in_both_colours() {
    for (colour, size) in COLOURS
        .into_iter()
        .flat_map(|colour| SIZES.into_iter().map(move |size| (colour, size)))
    {
        let shapes: Vec<Vec<u8>> = STATES
            .iter()
            .map(|state| alpha(*state, colour, size))
            .collect();
        let mut closest = usize::MAX;
        for (i, a) in shapes.iter().enumerate() {
            for b in &shapes[i + 1..] {
                let differing = a
                    .iter()
                    .zip(b)
                    .filter(|(x, y)| x.abs_diff(**y) >= 128)
                    .count();
                closest = closest.min(differing);
            }
        }
        // a pair that differed only in soft edge pixels would look the same in the
        // notification area, so the closest pair must differ in whole pixels.
        assert!(
            closest >= 12,
            "the closest {colour:?} pair at {size} px differs in {closest}"
        );
    }
}
