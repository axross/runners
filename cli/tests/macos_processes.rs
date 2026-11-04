//! the readers and the keep-awake hold, run as real processes against the fake
//! macOS programs, and the debug-only overrides that point the signals at them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::ffi::{OsStr, OsString};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use marshal_cli::macos::keep_awake::{
    CAFFEINATE_PROGRAM, HoldEvent, KeepAwake, KeepAwakeConfig, RETRY_INTERVAL,
};
use marshal_cli::macos::mac::MacSignals;
use marshal_cli::macos::network::{
    ARP_PROGRAM, HardwareAddress, NetworkReading, NetworkTools, PrimaryNetwork, ROUTE_PROGRAM,
};
use marshal_cli::macos::probe::{
    MAX_OUTPUT_BYTES, PROBE_BUDGET, ProbeError, resolve_program, run_bounded,
};
use marshal_cli::macos::signals::{SignalSource, Signals, resolve_scripted_directory};
use support::macos::{ARP_ROUTER, FAKE_MACOS, Fake, ROUTE_WIFI};

const WIFI_HARDWARE: [u8; 6] = [0xa4, 0x83, 0xe7, 1, 2, 3];

fn wifi() -> NetworkReading {
    NetworkReading::Primary(PrimaryNetwork {
        interface: "en0".to_string(),
        router: Some(Ipv4Addr::new(192, 168, 1, 1)),
        hardware: Some(HardwareAddress::new(WIFI_HARDWARE)),
    })
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn the_network_is_read_from_route_then_arp_with_their_exact_arguments() {
    let fake = Fake::wifi();
    assert_eq!(fake.tools().read().await, wifi());
    let calls = fake.invocations();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].arguments, words(&["-n", "get", "default"]));
    assert_eq!(calls[1].arguments, words(&["-n", "192.168.1.1"]));
}

#[tokio::test(flavor = "current_thread")]
async fn every_call_is_in_its_own_process_group_with_no_terminal_and_no_jit_variable() {
    let fake = Fake::wifi();
    let mut tools = fake.tools();
    tools.environment.push((
        OsString::from("ACTIONS_RUNNER_INPUT_JITCONFIG"),
        OsString::from("must-not-arrive"),
    ));
    tools.read().await;
    let calls = fake.invocations();
    assert_eq!(calls.len(), 2);
    for call in &calls {
        let what = format!("{:?}", call.arguments);
        assert!(!call.jit_present, "{what}");
        assert!(!call.stdin_terminal, "{what}");
        assert!(!call.stdout_terminal, "{what}");
        #[cfg(unix)]
        assert_eq!(
            call.pgid,
            Some(call.pid),
            "{what} is not its own process group leader"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_default_route_without_a_gateway_is_read_without_asking_arp() {
    let fake = Fake::new();
    fake.script(
        "route_stdout",
        "   route to: default\ndestination: default\n       mask: default\n  interface: utun4\n      flags: <UP,DONE,STATIC,GLOBAL>\n",
    );
    let reading = fake.tools().read().await;
    assert_eq!(
        reading,
        NetworkReading::Primary(PrimaryNetwork {
            interface: "utun4".to_string(),
            router: None,
            hardware: None,
        })
    );
    assert_eq!(fake.invocations().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_default_route_through_a_link_gateway_is_the_interface_without_a_router_and_asks_no_arp()
{
    let fake = Fake::new();
    fake.script(
        "route_stdout",
        "   route to: default\ndestination: default\n       mask: default\n    gateway: link#17\n  interface: utun4\n      flags: <UP,GATEWAY,DONE,STATIC,GLOBAL>\n",
    );
    assert_eq!(
        fake.tools().read().await,
        NetworkReading::Primary(PrimaryNetwork {
            interface: "utun4".to_string(),
            router: None,
            hardware: None,
        })
    );
    assert_eq!(fake.invocations().len(), 1, "arp was asked");
}

#[tokio::test(flavor = "current_thread")]
async fn a_routing_table_that_cannot_be_read_is_no_default_route() {
    type Script<'a> = &'a dyn Fn(&Fake);
    let missing_entry = |fake: &Fake| {
        fake.script("route_stdout", "");
        fake.script("route_exit", "1");
    };
    let garbage = |fake: &Fake| fake.script("route_stdout", "default via 10.0.0.1 dev en0\n");
    let failed = |fake: &Fake| fake.script("fail-route", "route: writing to routing socket");
    let flooded = |fake: &Fake| {
        fake.script("route_stdout", ROUTE_WIFI);
        fake.script("flood-route", "");
    };
    let good_text_failing_code = |fake: &Fake| {
        fake.script("route_stdout", ROUTE_WIFI);
        fake.script("route_exit", "1");
    };
    let cases: [(&str, Script); 5] = [
        ("an exit code of 1 and no output", &missing_entry),
        ("output in an unknown form", &garbage),
        ("a failure", &failed),
        ("output past the bound", &flooded),
        ("good output and a failing code", &good_text_failing_code),
    ];
    for (what, script) in cases {
        let fake = Fake::wifi();
        script(&fake);
        assert_eq!(
            fake.tools().read().await,
            NetworkReading::NoDefaultRoute,
            "{what}"
        );
        assert_eq!(fake.invocations().len(), 1, "{what}: arp was asked");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_route_program_that_is_missing_is_no_default_route() {
    let fake = Fake::wifi();
    let mut tools = fake.tools();
    tools.route = OsString::from("/nonexistent/marshal-test/route");
    assert_eq!(tools.read().await, NetworkReading::NoDefaultRoute);
}

#[tokio::test(flavor = "current_thread")]
async fn an_arp_that_cannot_be_read_leaves_the_hardware_address_unread_not_the_route() {
    let unread = PrimaryNetwork {
        interface: "en0".to_string(),
        router: Some(Ipv4Addr::new(192, 168, 1, 1)),
        hardware: None,
    };
    let cases = [
        (
            "an incomplete entry",
            "? (192.168.1.1) at (incomplete) on en0 ifscope [ethernet]\n",
            None,
        ),
        (
            "no entry",
            "192.168.1.1 (192.168.1.1) -- no entry\n",
            Some("1"),
        ),
        ("an unknown form", "surprise\n", None),
        (
            "another router",
            "? (10.0.0.1) at a4:83:e7:1:2:3 on en0 ifscope [ethernet]\n",
            None,
        ),
        (
            "another interface",
            "? (192.168.1.1) at a4:83:e7:1:2:3 on en1 ifscope [ethernet]\n",
            None,
        ),
        ("a failing code", ARP_ROUTER, Some("1")),
    ];
    for (what, text, code) in cases {
        let fake = Fake::wifi();
        fake.script("arp_stdout", text);
        if let Some(code) = code {
            fake.script("arp_exit", code);
        }
        assert_eq!(
            fake.tools().read().await,
            NetworkReading::Primary(unread.clone()),
            "{what}"
        );
    }
    let fake = Fake::wifi();
    fake.script("fail-arp", "");
    assert_eq!(
        fake.tools().read().await,
        NetworkReading::Primary(unread.clone())
    );
    let fake = Fake::wifi();
    let mut missing = fake.tools();
    missing.arp = OsString::from("/nonexistent/marshal-test/arp");
    let reading = missing.read().await;
    assert!(matches!(&reading, NetworkReading::Primary(read) if read.hardware.is_none()));
}

#[tokio::test(flavor = "current_thread")]
async fn arp_output_past_the_bound_is_refused() {
    assert_eq!(MAX_OUTPUT_BYTES, 64 * 1024);
    let fake = Fake::wifi();
    fake.script("flood-arp", "");
    let reading = fake.tools().read().await;
    assert!(matches!(&reading, NetworkReading::Primary(read) if read.hardware.is_none()));
    let program = OsString::from(FAKE_MACOS);
    let error = run_bounded(
        &program,
        &fake.environment(),
        &words(&["-n", "192.168.1.1"]),
        Duration::from_secs(10),
    )
    .await;
    assert!(matches!(error, Err(ProbeError::TooLong)), "{error:?}");
}

#[test]
fn each_call_is_bounded_to_two_seconds() {
    assert_eq!(PROBE_BUDGET, Duration::from_secs(2));
    assert_eq!(NetworkTools::default().budget, Duration::from_secs(2));
}

#[tokio::test(flavor = "current_thread")]
async fn a_route_that_hangs_ends_at_two_seconds_is_killed_and_reads_as_no_default_route() {
    let fake = Fake::wifi();
    fake.script("hang-route", "");
    let started = Instant::now();
    assert_eq!(fake.tools().read().await, NetworkReading::NoDefaultRoute);
    let took = started.elapsed();
    assert!(
        took >= Duration::from_secs(2) && took < Duration::from_secs(6),
        "{took:?}"
    );
    assert!(
        fake.ended_within(0, Duration::from_secs(10)),
        "route was not ended"
    );
    assert_eq!(fake.invocations().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn an_arp_that_hangs_ends_at_two_seconds_is_killed_and_leaves_the_address_unread() {
    let fake = Fake::wifi();
    fake.script("hang-arp", "");
    let started = Instant::now();
    let reading = fake.tools().read().await;
    let took = started.elapsed();
    assert!(matches!(&reading, NetworkReading::Primary(read) if read.hardware.is_none()));
    assert!(
        took >= Duration::from_secs(2) && took < Duration::from_secs(6),
        "{took:?}"
    );
    assert!(
        fake.ended_within(1, Duration::from_secs(10)),
        "arp was not ended"
    );
}

fn wait_for(deadline: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    done()
}

#[tokio::test(flavor = "current_thread")]
async fn the_hold_runs_caffeinate_on_this_process_and_release_ends_it() {
    let fake = Fake::new();
    let hold = KeepAwake::new(fake.keep_awake(RETRY_INTERVAL));
    assert_eq!(hold.hold(), HoldEvent::Unchanged);
    assert_eq!(hold.hold(), HoldEvent::Unchanged);
    assert_eq!(hold.poll(), HoldEvent::Unchanged);
    assert_eq!(hold.warning(), None);
    let calls = fake.invocations_after(1);
    assert_eq!(calls.len(), 1, "a second hold started a second caffeinate");
    assert_eq!(
        calls[0].arguments,
        words(&["-i", "-w", &std::process::id().to_string()])
    );
    assert!(!calls[0].stdin_terminal && !calls[0].stdout_terminal);
    assert!(!calls[0].jit_present);
    #[cfg(unix)]
    assert_eq!(calls[0].pgid, Some(calls[0].pid));

    hold.release();
    assert!(
        fake.ended_within(0, Duration::from_secs(10)),
        "release left caffeinate running"
    );
    assert_eq!(hold.poll(), HoldEvent::Unchanged);
    assert_eq!(hold.warning(), None);
    assert_eq!(fake.invocations().len(), 1, "a released hold started again");

    assert_eq!(hold.hold(), HoldEvent::Unchanged);
    assert_eq!(fake.invocations_after(2).len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn a_dropped_hold_ends_its_caffeinate() {
    let fake = Fake::new();
    let hold = KeepAwake::new(fake.keep_awake(RETRY_INTERVAL));
    hold.hold();
    assert_eq!(fake.invocations_after(1).len(), 1);
    drop(hold);
    assert!(fake.ended_within(0, Duration::from_secs(10)));
}

#[tokio::test(flavor = "current_thread")]
async fn the_jit_variable_never_reaches_caffeinate() {
    let fake = Fake::new();
    let mut config = fake.keep_awake(RETRY_INTERVAL);
    config.environment.push((
        OsString::from("ACTIONS_RUNNER_INPUT_JITCONFIG"),
        OsString::from("must-not-arrive"),
    ));
    let hold = KeepAwake::new(config);
    hold.hold();
    assert!(!fake.invocations_after(1)[0].jit_present);
}

#[tokio::test(flavor = "current_thread")]
async fn a_caffeinate_that_ends_early_warns_once_and_is_retried_after_the_interval() {
    let fake = Fake::new();
    fake.script("caffeinate_exit", "2");
    let hold = KeepAwake::new(fake.keep_awake(Duration::from_secs(1)));
    assert_eq!(hold.hold(), HoldEvent::Unchanged);

    let mut warned = None;
    assert!(wait_for(Duration::from_secs(10), || {
        match hold.poll() {
            HoldEvent::Warned(text) => {
                warned = Some(text);
                true
            }
            _ => false,
        }
    }));
    let text = warned.unwrap();
    assert!(text.contains("keep-awake is not held"), "{text}");
    assert!(text.contains("caffeinate ended early"), "{text}");
    assert!(
        text.contains("exit status: 2") || text.contains("exit code: 2"),
        "{text}"
    );
    assert_eq!(hold.warning(), Some(text));
    assert_eq!(fake.invocations_after(1).len(), 1);
    assert_eq!(hold.poll(), HoldEvent::Unchanged);
    assert_eq!(fake.invocations().len(), 1, "retried before the interval");

    fake.unscript("caffeinate_exit");
    assert!(wait_for(Duration::from_secs(10), || {
        hold.poll() == HoldEvent::Recovered
    }));
    assert_eq!(hold.warning(), None);
    assert_eq!(fake.invocations_after(2).len(), 2);
    assert_eq!(hold.poll(), HoldEvent::Unchanged);
    hold.release();
}

#[tokio::test(flavor = "current_thread")]
async fn a_caffeinate_that_keeps_ending_warns_only_once() {
    let fake = Fake::new();
    fake.script("caffeinate_exit", "1");
    let hold = KeepAwake::new(fake.keep_awake(Duration::from_millis(50)));
    hold.hold();
    let mut warnings = 0;
    for _ in 0..8 {
        std::thread::sleep(Duration::from_millis(300));
        if matches!(hold.poll(), HoldEvent::Warned(_)) {
            warnings += 1;
        }
    }
    assert_eq!(warnings, 1);
    assert!(fake.invocations().len() > 2, "no retry ran");
    assert!(hold.warning().is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn a_caffeinate_that_cannot_start_warns_at_once_and_is_retried() {
    let fake = Fake::new();
    let late = fake
        .dir
        .path()
        .join(if cfg!(windows) { "late.exe" } else { "late" });
    let mut config = fake.keep_awake(Duration::from_millis(200));
    config.program = late.clone().into_os_string();
    let hold = KeepAwake::new(config);
    let HoldEvent::Warned(text) = hold.hold() else {
        panic!("the hold did not warn");
    };
    assert!(text.contains("keep-awake is not held"), "{text}");
    assert!(text.contains("caffeinate could not start"), "{text}");
    assert_eq!(hold.warning(), Some(text));
    assert_eq!(hold.poll(), HoldEvent::Unchanged);

    std::fs::copy(FAKE_MACOS, &late).unwrap();
    assert!(wait_for(Duration::from_secs(10), || {
        hold.poll() == HoldEvent::Recovered
    }));
    assert_eq!(hold.warning(), None);
    assert_eq!(fake.invocations_after(1).len(), 1);
    hold.release();
}

#[test]
fn a_failed_hold_is_retried_at_a_bounded_interval() {
    assert_eq!(RETRY_INTERVAL, Duration::from_secs(10));
    assert_eq!(KeepAwakeConfig::default().retry_interval, RETRY_INTERVAL);
}

#[tokio::test(flavor = "current_thread")]
async fn the_real_source_reads_clocks_programs_and_the_hold_through_the_seam() {
    let fake = Fake::wifi();
    let source = MacSignals::new(fake.tools(), fake.keep_awake(RETRY_INTERVAL));
    assert!(!source.sleep_detected());
    assert!(!source.sleep_detected());
    assert_eq!(source.read_network().await, wifi());
    assert_eq!(source.hold_keep_awake(), HoldEvent::Unchanged);
    assert_eq!(source.poll_keep_awake(), HoldEvent::Unchanged);
    assert_eq!(source.keep_awake_warning(), None);
    assert_eq!(fake.invocations_after(3).len(), 3);
    source.release_keep_awake();
    assert!(fake.ended_within(2, Duration::from_secs(10)));
}

#[test]
fn a_release_build_runs_the_system_programs_by_absolute_path() {
    assert_eq!(ROUTE_PROGRAM, "/sbin/route");
    assert_eq!(ARP_PROGRAM, "/usr/sbin/arp");
    assert_eq!(CAFFEINATE_PROGRAM, "/usr/bin/caffeinate");
    let tools = NetworkTools::default();
    assert_eq!(tools.route, ROUTE_PROGRAM);
    assert_eq!(tools.arp, ARP_PROGRAM);
    assert_eq!(KeepAwakeConfig::default().program, CAFFEINATE_PROGRAM);
}

#[test]
fn the_program_overrides_name_the_program_in_a_debug_build_only() {
    let fake = OsStr::new("/tmp/fake-macos");
    for release in [ROUTE_PROGRAM, ARP_PROGRAM, CAFFEINATE_PROGRAM] {
        let resolved = resolve_program(Some(fake), release);
        if cfg!(debug_assertions) {
            assert_eq!(resolved, fake);
        } else {
            assert_eq!(resolved, release);
        }
        assert_eq!(resolve_program(None, release), release);
    }
}

#[test]
fn the_signal_directory_override_is_read_in_a_debug_build_only() {
    let directory = OsStr::new("/tmp/signals");
    let resolved = resolve_scripted_directory(Some(directory));
    if cfg!(debug_assertions) {
        assert_eq!(
            resolved.as_deref(),
            Some(std::path::Path::new("/tmp/signals"))
        );
    } else {
        assert_eq!(resolved, None);
    }
    assert_eq!(resolve_scripted_directory(None), None);
    assert_eq!(
        Signals::from_scripted_directory(Some(directory)).is_scripted(),
        cfg!(debug_assertions)
    );
    assert!(!Signals::from_scripted_directory(None).is_scripted());
}

#[cfg(debug_assertions)]
mod scripted {
    use super::*;
    use marshal_cli::macos::scripted::parse_scripted_network;

    fn source(fake: &Fake) -> Signals {
        Signals::from_scripted_directory(Some(fake.dir.path().as_os_str()))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_directory_drives_the_sleep_the_network_and_the_hold_from_outside() {
        let fake = Fake::new();
        let signals = source(&fake);
        assert!(signals.is_scripted());
        assert!(!signals.sleep_detected());
        fake.script("sleep", "");
        assert!(signals.sleep_detected());
        assert!(!signals.sleep_detected(), "the sleep was reported twice");
        assert!(!fake.dir.path().join("sleep").exists());

        assert_eq!(signals.read_network().await, NetworkReading::NoDefaultRoute);
        fake.script(
            "network",
            "interface=en0\nrouter=192.168.1.1\nhardware=a4:83:e7:1:2:3\n",
        );
        assert_eq!(signals.read_network().await, wifi());
        fake.script("network", "none");
        assert_eq!(signals.read_network().await, NetworkReading::NoDefaultRoute);

        let state = || std::fs::read_to_string(fake.dir.path().join("keep_awake")).unwrap();
        assert_eq!(signals.hold_keep_awake(), HoldEvent::Unchanged);
        assert_eq!(state(), "held");
        fake.script("keep_awake_failure", "scripted loss");
        assert_eq!(
            signals.poll_keep_awake(),
            HoldEvent::Warned("scripted loss".to_string())
        );
        assert_eq!(state(), "warning");
        assert_eq!(
            signals.keep_awake_warning().as_deref(),
            Some("scripted loss")
        );
        fake.unscript("keep_awake_failure");
        assert_eq!(signals.poll_keep_awake(), HoldEvent::Recovered);
        assert_eq!(state(), "held");
        signals.release_keep_awake();
        assert_eq!(state(), "released");
        assert_eq!(signals.keep_awake_warning(), None);
    }

    #[test]
    fn a_network_file_that_is_not_one_is_no_default_route() {
        assert_eq!(parse_scripted_network(""), NetworkReading::NoDefaultRoute);
        assert_eq!(
            parse_scripted_network("router=192.168.1.1"),
            NetworkReading::NoDefaultRoute
        );
        let bad_parts = parse_scripted_network("interface=utun1\nrouter=nope\nhardware=zz");
        assert_eq!(
            bad_parts,
            NetworkReading::Primary(PrimaryNetwork {
                interface: "utun1".to_string(),
                router: None,
                hardware: None,
            })
        );
    }
}
