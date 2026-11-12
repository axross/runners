//! when the icon's image is replaced.

use marshal_tray::{Look, TaskbarReading, Watch, bitmap, size_for_scale};
use marshal_view::{GlyphColour, IconState};

fn polled(watch: &mut Watch, value: u32, scale: f64) -> Option<Look> {
    watch
        .poll(TaskbarReading::Value(value), Some(scale))
        .replace
}

#[test]
fn the_first_poll_puts_an_image_on_the_icon() {
    let mut watch = Watch::new();
    let look = polled(&mut watch, 0, 1.0).expect("an image");
    assert_eq!(look.state, IconState::NotInControl);
    assert_eq!(look.colour, GlyphColour::White);
    assert_eq!(look.size, 16);
}

#[test]
fn nothing_is_replaced_while_nothing_changes() {
    let mut watch = Watch::new();
    polled(&mut watch, 0, 1.0);
    assert_eq!(polled(&mut watch, 0, 1.0), None);
    assert_eq!(polled(&mut watch, 0, 1.0), None);
}

#[test]
fn a_mode_change_replaces_the_image_with_the_other_colour() {
    let mut watch = Watch::new();
    polled(&mut watch, 0, 1.0);
    let look = polled(&mut watch, 1, 1.0).expect("an image");
    assert_eq!(look.colour, GlyphColour::NearBlack);
    let back = polled(&mut watch, 0, 1.0).expect("an image");
    assert_eq!(back.colour, GlyphColour::White);
}

#[test]
fn a_scaling_change_replaces_the_image_with_one_of_the_size() {
    let mut watch = Watch::new();
    polled(&mut watch, 0, 1.0);
    assert_eq!(polled(&mut watch, 0, 1.5).expect("an image").size, 24);
    assert_eq!(polled(&mut watch, 0, 2.0).expect("an image").size, 32);
    assert_eq!(polled(&mut watch, 0, 2.0), None);
}

#[test]
fn a_state_change_replaces_the_image_once() {
    let mut watch = Watch::new();
    polled(&mut watch, 0, 1.0);
    assert!(watch.set_state(IconState::AllUp).is_some());
    assert_eq!(watch.set_state(IconState::AllUp), None);
    assert_eq!(polled(&mut watch, 0, 1.0), None);
}

#[test]
fn an_absent_mode_counts_as_dark_and_is_logged_once() {
    let mut watch = Watch::new();
    let first = watch.poll(TaskbarReading::Absent, Some(1.0));
    assert_eq!(first.replace.expect("an image").colour, GlyphColour::White);
    assert!(first.warn.is_some());
    assert_eq!(watch.poll(TaskbarReading::Absent, Some(1.0)).warn, None);
}

#[test]
fn an_unreadable_mode_counts_as_dark_and_is_logged_once() {
    let mut watch = Watch::new();
    polled(&mut watch, 1, 1.0);
    let failed = watch.poll(TaskbarReading::Failed("denied".to_string()), Some(1.0));
    assert_eq!(failed.replace.expect("an image").colour, GlyphColour::White);
    assert!(failed.warn.expect("a line").contains("denied"));
    assert_eq!(
        watch
            .poll(TaskbarReading::Failed("denied".to_string()), Some(1.0))
            .warn,
        None
    );
}

#[test]
fn a_value_that_is_neither_zero_nor_one_counts_as_dark() {
    let mut watch = Watch::new();
    let polled = watch.poll(TaskbarReading::Value(7), Some(1.0));
    assert_eq!(polled.replace.expect("an image").colour, GlyphColour::White);
}

#[test]
fn a_display_whose_scaling_cannot_be_read_keeps_the_last_size() {
    let mut watch = Watch::new();
    polled(&mut watch, 0, 2.0);
    assert_eq!(watch.poll(TaskbarReading::Value(0), None).replace, None);
    assert_eq!(
        watch.poll(TaskbarReading::Value(0), Some(f64::NAN)).replace,
        None
    );
    assert_eq!(
        watch.poll(TaskbarReading::Value(0), Some(0.0)).replace,
        None
    );
}

#[test]
fn the_sizes_follow_the_scalings_windows_offers() {
    for (scale, size) in [
        (1.0, 16),
        (1.25, 20),
        (1.5, 24),
        (1.75, 28),
        (2.0, 32),
        (3.0, 32),
    ] {
        assert_eq!(size_for_scale(scale), size, "at {scale}");
    }
}

#[test]
fn the_image_takes_the_colour_of_the_glyph() {
    let white = bitmap(IconState::AllUp, GlyphColour::White, 16);
    let dark = bitmap(IconState::AllUp, GlyphColour::NearBlack, 16);
    let solid = |image: &marshal_tray::Bitmap| {
        image
            .rgba
            .chunks(4)
            .find(|p| p[3] == 255)
            .map(|p| [p[0], p[1], p[2]])
    };
    assert_eq!(solid(&white), Some([255, 255, 255]));
    assert_eq!(solid(&dark), Some([0x1f, 0x1f, 0x1f]));
}

#[test]
fn what_is_applied_later_is_the_newest_look_decided_whatever_order_looks_are_queued_in() {
    let mut watch = Watch::new();
    polled(&mut watch, 0, 1.0);
    // the view's thread decides one look and the polling thread a newer one, and the
    // older look reaches the event loop last
    let older = watch.set_state(IconState::AllUp).expect("an image");
    let newer = polled(&mut watch, 1, 1.0).expect("an image");
    assert_ne!(older, newer);
    assert_eq!(
        watch.current(),
        newer,
        "the older look would be applied last"
    );
    assert_eq!(watch.current().state, IconState::AllUp);
    assert_eq!(watch.current().colour, GlyphColour::NearBlack);
}

#[test]
fn the_current_look_is_the_one_the_last_decision_returned_and_changes_nothing() {
    let mut watch = Watch::new();
    let decided = polled(&mut watch, 0, 1.5).expect("an image");
    assert_eq!(watch.current(), decided);
    assert_eq!(watch.current(), decided);
    assert_eq!(
        polled(&mut watch, 0, 1.5),
        None,
        "reading it records nothing"
    );
}
