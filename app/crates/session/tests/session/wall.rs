//! the machine's clock.

use std::sync::atomic::{AtomicUsize, Ordering};
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

static OFFSET_READS: AtomicUsize = AtomicUsize::new(0);

fn counted_offset() -> i32 {
    OFFSET_READS.fetch_add(1, Ordering::SeqCst);
    0
}

#[test]
fn the_time_alone_reads_no_offset_and_a_clock_reads_it_once() {
    let wall = WallClock::new(counted_offset);
    let _ = wall.now_ms();
    assert_eq!(OFFSET_READS.load(Ordering::SeqCst), 0);
    let _ = wall.clock();
    assert_eq!(OFFSET_READS.load(Ordering::SeqCst), 1);
}
