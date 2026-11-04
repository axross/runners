//! the judgments the signals make without a process: the sleep, the readers' parsers,
//! the primary network's state machine and the reason texts.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::net::Ipv4Addr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(debug_assertions)]
use marshal_cli::macos::fake::FakeSignals;
#[cfg(debug_assertions)]
use marshal_cli::macos::keep_awake::HoldEvent;
use marshal_cli::macos::network::{
    HardwareAddress, NetworkReading, PrimaryNetwork, RouteEntry, parse_arp, parse_hardware_address,
    parse_route,
};
use marshal_cli::macos::reason::{network_change_reason, sleep_reason};
#[cfg(debug_assertions)]
use marshal_cli::macos::signals::SignalSource;
use marshal_cli::macos::sleep::{ClockSample, SLEEP_GAP, SleepDetector, slept_between};
use marshal_cli::macos::watch::{NetworkChange, NetworkWatch, WatchOutcome};
use support::macos::{ARP_ROUTER, ROUTE_WIFI};

fn millis(count: u64) -> Duration {
    Duration::from_millis(count)
}

struct Clocks {
    wall: SystemTime,
    monotonic: Instant,
}

impl Clocks {
    fn new() -> Self {
        Self {
            wall: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            monotonic: Instant::now(),
        }
    }

    fn sample(&self) -> ClockSample {
        ClockSample {
            wall: self.wall,
            monotonic: self.monotonic,
        }
    }

    /// both clocks move `monotonic`, and the wall clock moves `wall_extra` more.
    fn advance(&mut self, monotonic: Duration, wall_extra: Duration) -> ClockSample {
        self.monotonic += monotonic;
        self.wall += monotonic + wall_extra;
        self.sample()
    }

    fn step_back(&mut self, monotonic: Duration, wall_back: Duration) -> ClockSample {
        self.monotonic += monotonic;
        self.wall = self.wall + monotonic - wall_back;
        self.sample()
    }
}

#[test]
fn a_gap_of_exactly_three_seconds_is_not_a_sleep_and_a_little_more_is() {
    assert_eq!(SLEEP_GAP, Duration::from_secs(3));
    let mut clocks = Clocks::new();
    let before = clocks.sample();
    let ordinary = clocks.advance(Duration::from_secs(1), Duration::ZERO);
    assert!(!slept_between(&before, &ordinary));
    let edge = clocks.advance(Duration::from_secs(1), SLEEP_GAP);
    assert!(!slept_between(&ordinary, &edge));
    let over = clocks.advance(Duration::from_secs(1), SLEEP_GAP + Duration::from_nanos(1));
    assert!(slept_between(&edge, &over));
}

#[test]
fn a_sleep_is_the_wall_clock_moving_while_the_monotonic_clock_stands_still() {
    let mut clocks = Clocks::new();
    let before = clocks.sample();
    let after = clocks.advance(Duration::ZERO, Duration::from_secs(3600));
    assert!(slept_between(&before, &after));
}

#[test]
fn a_wall_clock_step_back_of_the_same_size_counts_and_one_of_exactly_the_gap_does_not() {
    let mut clocks = Clocks::new();
    let before = clocks.sample();
    let at_the_edge = clocks.step_back(Duration::from_secs(1), SLEEP_GAP);
    assert!(!slept_between(&before, &at_the_edge));
    let over = clocks.step_back(Duration::from_secs(1), SLEEP_GAP + millis(1));
    assert!(slept_between(&at_the_edge, &over));
    let hour = clocks.step_back(Duration::from_secs(1), Duration::from_secs(3600));
    assert!(slept_between(&over, &hour));
}

#[test]
fn the_gap_is_between_the_two_clocks_not_the_time_between_samples() {
    let mut clocks = Clocks::new();
    let before = clocks.sample();
    let slow_tick = clocks.advance(Duration::from_secs(60), Duration::ZERO);
    assert!(!slept_between(&before, &slow_tick));
}

#[test]
fn the_detector_sets_a_baseline_then_reports_each_sleep_once() {
    let mut clocks = Clocks::new();
    let mut detector = SleepDetector::new();
    assert!(!detector.observe(clocks.sample()));
    assert!(!detector.observe(clocks.advance(Duration::from_secs(1), Duration::ZERO)));
    assert!(detector.observe(clocks.advance(Duration::from_secs(1), Duration::from_secs(600))));
    assert!(!detector.observe(clocks.advance(Duration::from_secs(1), Duration::ZERO)));
    assert!(detector.observe(clocks.advance(Duration::ZERO, Duration::from_secs(10))));
}

#[test]
fn the_first_sample_of_a_detector_is_never_a_sleep() {
    let mut detector = SleepDetector::new();
    let far_wall = ClockSample {
        wall: UNIX_EPOCH,
        monotonic: Instant::now(),
    };
    assert!(!detector.observe(far_wall));
}

#[test]
fn route_is_read_as_macos_prints_it() {
    assert_eq!(
        parse_route(ROUTE_WIFI),
        Some(RouteEntry {
            interface: "en0".to_string(),
            gateway: Some(Ipv4Addr::new(192, 168, 1, 1)),
        })
    );
}

#[test]
fn a_route_that_leaves_through_a_point_to_point_interface_has_no_gateway() {
    let vpn = "   route to: default
destination: default
       mask: default
  interface: utun4
      flags: <UP,DONE,STATIC,GLOBAL>
";
    assert_eq!(
        parse_route(vpn),
        Some(RouteEntry {
            interface: "utun4".to_string(),
            gateway: None,
        })
    );
}

#[test]
fn route_output_in_any_other_form_is_refused() {
    let with = |old: &str, new: &str| ROUTE_WIFI.replacen(old, new, 1);
    let refused = [
        String::new(),
        "route: writing to routing socket: not in table\n".to_string(),
        "   route to: default\n".to_string(),
        with("route to: default", "route to: 8.8.8.8"),
        with("gateway: 192.168.1.1", "gateway: fe80::1%en0"),
        with("gateway: 192.168.1.1", "gateway: link#17"),
        with("gateway: 192.168.1.1", "gateway: 0.0.0.0"),
        with("gateway: 192.168.1.1", "gateway: 192.168.001.001"),
        with("gateway: 192.168.1.1", "gateway: 192.168.1.1 extra"),
        with("interface: en0", "interface: "),
        with("interface: en0", "interface: 0en"),
        with("interface: en0", "interface: en0 en1"),
        with("interface: en0", "interface: abcdefghijklmnop"),
        with("interface: en0", "interface: en0-x"),
        ROUTE_WIFI.replace("  interface: en0\n", ""),
        format!("{ROUTE_WIFI}  interface: en1\n"),
        format!("{ROUTE_WIFI}    gateway: 192.168.1.2\n"),
        ROUTE_WIFI.replacen("   route to: default\n", "", 1),
    ];
    for text in refused {
        assert_eq!(parse_route(&text), None, "{text:?}");
    }
}

#[test]
fn the_longest_interface_name_is_fifteen_characters() {
    let name = "abcdefghijklmno";
    let text = ROUTE_WIFI.replacen("en0", name, 1);
    assert_eq!(parse_route(&text).unwrap().interface, name);
}

const ROUTER: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);

fn address(bytes: [u8; 6]) -> HardwareAddress {
    HardwareAddress::new(bytes)
}

#[test]
fn arp_is_read_as_macos_prints_it_without_zero_padding() {
    assert_eq!(
        parse_arp(ARP_ROUTER, ROUTER, "en0"),
        Some(address([0xa4, 0x83, 0xe7, 1, 2, 3]))
    );
    let permanent = "? (192.168.1.1) at 0:a:95:ff:0:10 on en0 ifscope permanent [ethernet]\n";
    assert_eq!(
        parse_arp(permanent, ROUTER, "en0"),
        Some(address([0, 0x0a, 0x95, 0xff, 0, 0x10]))
    );
    let proxy =
        "? (192.168.1.1) at a4:83:e7:1:2:3 on en0 ifscope published (proxy only) [ethernet]";
    assert!(parse_arp(proxy, ROUTER, "en0").is_some());
}

#[test]
fn hardware_addresses_print_padded() {
    let parsed = parse_hardware_address("0:a:95:ff:0:10").unwrap();
    assert_eq!(parsed.to_string(), "00:0a:95:ff:00:10");
}

#[test]
fn an_incomplete_or_missing_arp_entry_means_the_address_cannot_be_read() {
    let incomplete = "? (192.168.1.1) at (incomplete) on en0 ifscope [ethernet]\n";
    assert_eq!(parse_arp(incomplete, ROUTER, "en0"), None);
    let missing = "192.168.1.1 (192.168.1.1) -- no entry\n";
    assert_eq!(parse_arp(missing, ROUTER, "en0"), None);
    assert_eq!(parse_arp("", ROUTER, "en0"), None);
}

#[test]
fn arp_output_in_any_other_form_is_refused() {
    let refused = [
        "? (192.168.1.1) at A4:83:E7:1:2:3 on en0 ifscope [ethernet]",
        "? (192.168.1.1) at a4:83:e7:1:2 on en0 ifscope [ethernet]",
        "? (192.168.1.1) at a4:83:e7:1:2:3:4 on en0 ifscope [ethernet]",
        "? (192.168.1.1) at a4:83:e7:100:2:3 on en0 ifscope [ethernet]",
        "? (192.168.1.1) at a4:83:e7:1:2:g on en0 ifscope [ethernet]",
        "? (192.168.1.1) at a4-83-e7-1-2-3 on en0 ifscope [ethernet]",
        "? (192.168.1.1) at a4:83:e7:1:2:3",
        "? (192.168.1.2) at a4:83:e7:1:2:3 on en0 ifscope [ethernet]",
        "? (192.168.1.1 at a4:83:e7:1:2:3 on en0 ifscope [ethernet]",
        "router (192.168.1.1) at a4:83:e7:1:2:3 on en0 ifscope [ethernet]",
        "? (192.168.1.1) at a4:83:e7:1:2:3 on en0 ifscope surprise [ethernet]",
        "? (192.168.1.1) at a4:83:e7:1:2:3 on 0en ifscope [ethernet]",
        "arp: bad address",
    ];
    for text in refused {
        assert_eq!(parse_arp(text, ROUTER, "en0"), None, "{text:?}");
    }
}

#[test]
fn one_unreadable_line_refuses_the_whole_arp_answer() {
    let text = format!("{ARP_ROUTER}something else\n");
    assert_eq!(parse_arp(&text, ROUTER, "en0"), None);
}

#[test]
fn arp_entries_on_other_interfaces_are_ignored_and_disagreeing_ones_refused() {
    let both = "? (192.168.1.1) at a4:83:e7:1:2:3 on en0 ifscope [ethernet]
? (192.168.1.1) at 0:11:22:33:44:55 on en1 ifscope [ethernet]
";
    assert_eq!(
        parse_arp(both, ROUTER, "en0"),
        Some(address([0xa4, 0x83, 0xe7, 1, 2, 3]))
    );
    assert_eq!(
        parse_arp(both, ROUTER, "en1"),
        Some(address([0, 0x11, 0x22, 0x33, 0x44, 0x55]))
    );
    assert_eq!(parse_arp(both, ROUTER, "en2"), None);
    let disagree = "? (192.168.1.1) at a4:83:e7:1:2:3 on en0 ifscope [ethernet]
? (192.168.1.1) at 0:11:22:33:44:55 on en0 [ethernet]
";
    assert_eq!(parse_arp(disagree, ROUTER, "en0"), None);
    let agree = "? (192.168.1.1) at a4:83:e7:1:2:3 on en0 ifscope [ethernet]
? (192.168.1.1) at a4:83:e7:1:2:3 on en0 [ethernet]
";
    assert!(parse_arp(agree, ROUTER, "en0").is_some());
}

fn network(interface: &str, router: Option<[u8; 4]>, hardware: Option<[u8; 6]>) -> PrimaryNetwork {
    PrimaryNetwork {
        interface: interface.to_string(),
        router: router.map(Ipv4Addr::from),
        hardware: hardware.map(HardwareAddress::new),
    }
}

const ROUTER_BYTES: Option<[u8; 4]> = Some([192, 168, 1, 1]);
const MAC_A: Option<[u8; 6]> = Some([0xa4, 0x83, 0xe7, 1, 2, 3]);
const MAC_B: Option<[u8; 6]> = Some([0, 0x11, 0x22, 0x33, 0x44, 0x55]);

fn wifi() -> NetworkReading {
    NetworkReading::Primary(network("en0", ROUTER_BYTES, MAC_A))
}

fn changed(from: &str, to: &str) -> WatchOutcome {
    WatchOutcome::Changed(NetworkChange {
        from: from.to_string(),
        to: to.to_string(),
    })
}

#[test]
fn a_network_at_start_is_recorded_at_once_and_the_same_network_is_steady() {
    let mut watch = NetworkWatch::begin(wifi());
    assert!(!watch.is_waiting());
    assert_eq!(watch.recorded(), Some(&network("en0", ROUTER_BYTES, MAC_A)));
    for _ in 0..5 {
        assert_eq!(watch.observe(wifi()), WatchOutcome::Steady);
    }
}

#[test]
fn a_difference_on_one_check_is_not_a_change_and_a_return_resets_the_count() {
    let mut watch = NetworkWatch::begin(wifi());
    let other = NetworkReading::Primary(network("en1", ROUTER_BYTES, MAC_A));
    assert_eq!(watch.observe(other.clone()), WatchOutcome::Differing);
    assert_eq!(watch.observe(wifi()), WatchOutcome::Steady);
    assert_eq!(watch.observe(other.clone()), WatchOutcome::Differing);
    assert_eq!(watch.observe(other), changed("en0", "en1"));
}

#[test]
fn joining_another_network_on_the_same_interface_is_a_change_by_its_router() {
    let mut watch = NetworkWatch::begin(wifi());
    let other_router = NetworkReading::Primary(network("en0", Some([10, 0, 0, 1]), MAC_A));
    assert_eq!(watch.observe(other_router.clone()), WatchOutcome::Differing);
    assert_eq!(watch.observe(other_router), changed("en0", "en0"));
}

#[test]
fn another_router_hardware_address_behind_the_same_address_is_a_change() {
    let mut watch = NetworkWatch::begin(wifi());
    let impostor = NetworkReading::Primary(network("en0", ROUTER_BYTES, MAC_B));
    assert_eq!(watch.observe(impostor.clone()), WatchOutcome::Differing);
    assert_eq!(watch.observe(impostor), changed("en0", "en0"));
}

#[test]
fn roaming_between_access_points_of_one_network_is_never_a_change() {
    let mut watch = NetworkWatch::begin(wifi());
    for _ in 0..10 {
        assert_eq!(watch.observe(wifi()), WatchOutcome::Steady);
    }
}

#[test]
fn an_unreadable_hardware_address_is_not_compared_until_it_can_be_read() {
    let mut watch = NetworkWatch::begin(wifi());
    let unreadable = NetworkReading::Primary(network("en0", ROUTER_BYTES, None));
    for _ in 0..5 {
        assert_eq!(watch.observe(unreadable.clone()), WatchOutcome::Steady);
    }
    assert_eq!(watch.observe(wifi()), WatchOutcome::Steady);

    let mut unread_at_start = NetworkWatch::begin(unreadable.clone());
    assert_eq!(unread_at_start.observe(wifi()), WatchOutcome::Steady);
    let impostor = NetworkReading::Primary(network("en0", ROUTER_BYTES, MAC_B));
    assert_eq!(
        unread_at_start.observe(impostor.clone()),
        WatchOutcome::Differing
    );
    assert_eq!(unread_at_start.observe(impostor), changed("en0", "en0"));
}

#[test]
fn a_vpn_taking_the_default_route_is_a_change_switched_on_or_off() {
    let vpn = NetworkReading::Primary(network("utun4", None, None));
    let mut on = NetworkWatch::begin(wifi());
    assert_eq!(on.observe(vpn.clone()), WatchOutcome::Differing);
    assert_eq!(on.observe(vpn.clone()), changed("en0", "utun4"));

    let mut off = NetworkWatch::begin(vpn);
    assert_eq!(off.observe(wifi()), WatchOutcome::Differing);
    assert_eq!(off.observe(wifi()), changed("utun4", "en0"));
}

#[test]
fn losing_the_default_route_for_two_checks_is_a_change_to_none() {
    let mut watch = NetworkWatch::begin(wifi());
    assert_eq!(
        watch.observe(NetworkReading::NoDefaultRoute),
        WatchOutcome::Differing
    );
    assert_eq!(
        watch.observe(NetworkReading::NoDefaultRoute),
        changed("en0", "none")
    );
    let mut brief = NetworkWatch::begin(wifi());
    assert_eq!(
        brief.observe(NetworkReading::NoDefaultRoute),
        WatchOutcome::Differing
    );
    assert_eq!(brief.observe(wifi()), WatchOutcome::Steady);
}

#[test]
fn two_differing_checks_count_even_when_they_differ_from_each_other() {
    let mut watch = NetworkWatch::begin(wifi());
    let other = NetworkReading::Primary(network("en1", ROUTER_BYTES, MAC_A));
    assert_eq!(watch.observe(other), WatchOutcome::Differing);
    assert_eq!(
        watch.observe(NetworkReading::NoDefaultRoute),
        changed("en0", "none")
    );
}

#[test]
fn a_change_is_final() {
    let mut watch = NetworkWatch::begin(wifi());
    watch.observe(NetworkReading::NoDefaultRoute);
    let outcome = watch.observe(NetworkReading::NoDefaultRoute);
    assert_eq!(watch.observe(wifi()), outcome);
    assert_eq!(watch.recorded(), None);
}

#[test]
fn with_no_default_route_at_start_the_watch_waits_until_a_network_held_for_two_checks() {
    let mut watch = NetworkWatch::begin(NetworkReading::NoDefaultRoute);
    assert!(watch.is_waiting());
    assert_eq!(watch.recorded(), None);
    assert_eq!(
        watch.observe(NetworkReading::NoDefaultRoute),
        WatchOutcome::Waiting
    );
    assert_eq!(watch.observe(wifi()), WatchOutcome::Waiting);
    assert!(watch.is_waiting());
    assert_eq!(
        watch.observe(wifi()),
        WatchOutcome::Recorded(network("en0", ROUTER_BYTES, MAC_A))
    );
    assert!(!watch.is_waiting());
    assert_eq!(watch.observe(wifi()), WatchOutcome::Steady);
}

#[test]
fn a_network_that_does_not_hold_for_two_checks_in_a_row_is_not_recorded() {
    let mut watch = NetworkWatch::begin(NetworkReading::NoDefaultRoute);
    assert_eq!(watch.observe(wifi()), WatchOutcome::Waiting);
    assert_eq!(
        watch.observe(NetworkReading::NoDefaultRoute),
        WatchOutcome::Waiting
    );
    assert_eq!(watch.observe(wifi()), WatchOutcome::Waiting);
    let other = NetworkReading::Primary(network("en1", ROUTER_BYTES, MAC_A));
    assert_eq!(watch.observe(other.clone()), WatchOutcome::Waiting);
    assert!(watch.is_waiting());
    assert_eq!(
        watch.observe(other),
        WatchOutcome::Recorded(network("en1", ROUTER_BYTES, MAC_A))
    );
}

#[test]
fn a_waiting_watch_holds_a_network_by_interface_and_router_when_its_hardware_address_is_unread() {
    let mut watch = NetworkWatch::begin(NetworkReading::NoDefaultRoute);
    let unread = NetworkReading::Primary(network("en0", ROUTER_BYTES, None));
    assert_eq!(watch.observe(unread), WatchOutcome::Waiting);
    assert!(matches!(watch.observe(wifi()), WatchOutcome::Recorded(_)));
}

#[test]
fn the_reasons_are_exact_and_name_interfaces_only() {
    assert_eq!(sleep_reason(), "the Mac slept, so every runner was stopped");
    assert_eq!(
        network_change_reason("en0", "en1"),
        "the primary network changed (en0 to en1), so every runner was stopped"
    );
    let lost = NetworkChange {
        from: "en0".to_string(),
        to: "none".to_string(),
    };
    assert_eq!(
        lost.reason(),
        "the primary network changed (en0 to none), so every runner was stopped"
    );
    let mut watch = NetworkWatch::begin(wifi());
    watch.observe(NetworkReading::NoDefaultRoute);
    let WatchOutcome::Changed(change) = watch.observe(NetworkReading::NoDefaultRoute) else {
        panic!("not a change");
    };
    let reason = change.reason();
    assert!(!reason.contains("192.168"), "{reason}");
    assert!(!reason.contains("a4:83"), "{reason}");
}

#[cfg(debug_assertions)]
#[tokio::test(flavor = "current_thread")]
async fn the_fake_source_answers_as_scripted() {
    let fake = FakeSignals::new();
    assert!(!fake.sleep_detected());
    fake.sleep();
    assert!(fake.sleep_detected());
    assert!(!fake.sleep_detected());

    assert_eq!(fake.read_network().await, NetworkReading::NoDefaultRoute);
    fake.script_network([wifi(), NetworkReading::NoDefaultRoute]);
    assert_eq!(fake.read_network().await, wifi());
    assert_eq!(fake.read_network().await, NetworkReading::NoDefaultRoute);
    assert_eq!(fake.read_network().await, NetworkReading::NoDefaultRoute);
    assert_eq!(fake.network_reads(), 4);
}

#[cfg(debug_assertions)]
#[test]
fn the_fake_hold_warns_once_and_recovers() {
    let fake = FakeSignals::new();
    assert_eq!(fake.hold_keep_awake(), HoldEvent::Unchanged);
    assert!(fake.is_held());
    fake.fail_keep_awake(Some("lost"));
    assert_eq!(
        fake.poll_keep_awake(),
        HoldEvent::Warned("lost".to_string())
    );
    assert_eq!(fake.poll_keep_awake(), HoldEvent::Unchanged);
    assert_eq!(fake.keep_awake_warning().as_deref(), Some("lost"));
    assert!(!fake.is_held());
    fake.fail_keep_awake(None);
    assert_eq!(fake.poll_keep_awake(), HoldEvent::Recovered);
    assert_eq!(fake.keep_awake_warning(), None);
    fake.release_keep_awake();
    assert!(!fake.is_held());
    assert_eq!(fake.poll_keep_awake(), HoldEvent::Unchanged);
    assert_eq!((fake.hold_calls(), fake.release_calls()), (1, 1));
}

#[cfg(debug_assertions)]
#[test]
fn a_fake_hold_that_cannot_be_taken_warns_at_once() {
    let fake = FakeSignals::new();
    fake.fail_keep_awake(Some("no caffeinate"));
    assert_eq!(
        fake.hold_keep_awake(),
        HoldEvent::Warned("no caffeinate".to_string())
    );
    assert_eq!(fake.keep_awake_warning().as_deref(), Some("no caffeinate"));
}

#[cfg(debug_assertions)]
#[test]
fn a_shared_source_is_a_source() {
    fn sleeps(source: &impl SignalSource) -> bool {
        source.sleep_detected()
    }
    let shared = std::rc::Rc::new(FakeSignals::new());
    shared.sleep();
    assert!(sleeps(&shared));
}
