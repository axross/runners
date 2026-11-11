//! the machine's clock.

use std::time::{SystemTime, UNIX_EPOCH};

use marshal_session::{TimeSource, WallClock};

fn nine_hours_ahead() -> i32 {
    540
}

#[test]
fn the_clock_is_the_systems_with_the_offset_read_each_time() {
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let clock = WallClock::new(nine_hours_ahead).clock();
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    assert_eq!(clock.utc_offset_minutes, 540);
    assert!((before..=after).contains(&u128::from(clock.now_ms)));
}
