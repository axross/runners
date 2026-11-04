//! the primary network: the interface that holds the IPv4 default route, that
//! route's router and the router's hardware address, read from `route -n get default`
//! and `arp -n <router>`. Both readers accept only the forms macOS prints and refuse
//! everything else.

use std::ffi::OsString;
use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;
use std::time::Duration;

use super::probe::{PROBE_BUDGET, resolve_program, run_bounded};

/// the programs a release build runs, by absolute path, so a changed `PATH` cannot
/// replace them.
pub const ROUTE_PROGRAM: &str = "/sbin/route";
pub const ARP_PROGRAM: &str = "/usr/sbin/arp";

/// the variables that name `route` and `arp` in a debug build; a release build
/// ignores them.
pub const TEST_ROUTE_VARIABLE: &str = "MARSHAL_TEST_ROUTE";
pub const TEST_ARP_VARIABLE: &str = "MARSHAL_TEST_ARP";

const LONGEST_INTERFACE_NAME: usize = 15;

/// a router's hardware address, six bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwareAddress([u8; 6]);

impl HardwareAddress {
    #[must_use]
    pub fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for HardwareAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}

/// a hardware address as `arp` prints it: six lowercase hexadecimal bytes of one or
/// two digits, separated by colons, as `%x` writes them. Anything else is refused.
#[must_use]
pub fn parse_hardware_address(text: &str) -> Option<HardwareAddress> {
    let mut bytes = [0_u8; 6];
    let mut parts = text.split(':');
    for slot in &mut bytes {
        let part = parts.next()?;
        let valid = (1..=2).contains(&part.len())
            && part
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !valid {
            return None;
        }
        *slot = u8::from_str_radix(part, 16).ok()?;
    }
    parts.next().is_none().then_some(HardwareAddress(bytes))
}

/// the interface that holds the default route, with the router and its hardware
/// address when they are known. A default route that leaves through a point-to-point
/// interface, as a VPN's does, has no router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryNetwork {
    pub interface: String,
    pub router: Option<Ipv4Addr>,
    pub hardware: Option<HardwareAddress>,
}

impl PrimaryNetwork {
    /// whether both are the same network: the same interface and router, and the same
    /// hardware address unless one of them could not be read.
    #[must_use]
    pub fn is_same_network_as(&self, other: &Self) -> bool {
        self.interface == other.interface
            && self.router == other.router
            && match (self.hardware, other.hardware) {
                (Some(ours), Some(theirs)) => ours == theirs,
                _ => true,
            }
    }
}

/// one reading of the routing table. A table that cannot be read is
/// `NoDefaultRoute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkReading {
    Primary(PrimaryNetwork),
    NoDefaultRoute,
}

/// what `route -n get default` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEntry {
    pub interface: String,
    pub gateway: Option<Ipv4Addr>,
}

fn is_interface_name(name: &str) -> bool {
    let mut characters = name.chars();
    name.len() <= LONGEST_INTERFACE_NAME
        && characters.next().is_some_and(|c| c.is_ascii_alphabetic())
        && characters.all(|c| c.is_ascii_alphanumeric())
}

/// the interface and gateway of `route -n get default`. The first line must be
/// `route to: default`; `interface:` must appear once and name an interface;
/// `gateway:` appears once at most, as a routable IPv4 address, and is absent from a
/// route that leaves through a point-to-point interface. Any other form is refused.
#[must_use]
pub fn parse_route(output: &str) -> Option<RouteEntry> {
    let mut lines = output.lines();
    let (key, value) = lines.next()?.split_once(':')?;
    if key.trim() != "route to" || value.trim() != "default" {
        return None;
    }
    let (mut interface, mut gateway) = (None, None);
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let slot = match key.trim() {
            "interface" => &mut interface,
            "gateway" => &mut gateway,
            _ => continue,
        };
        if slot.replace(value.trim()).is_some() {
            return None;
        }
    }
    let interface = interface.filter(|name| is_interface_name(name))?;
    let gateway = match gateway {
        None => None,
        Some(text) => Some(
            Ipv4Addr::from_str(text)
                .ok()
                .filter(|a| !a.is_unspecified())?,
        ),
    };
    Some(RouteEntry {
        interface: interface.to_string(),
        gateway,
    })
}

const ARP_FLAGS: [&str; 5] = ["ifscope", "permanent", "published", "(proxy", "only)"];

/// one entry of `arp -n <router>`: `? (<router>) at <address> on <interface> ...`.
enum ArpEntry<'a> {
    Complete {
        interface: &'a str,
        hardware: HardwareAddress,
    },
    Incomplete,
}

fn parse_arp_line(line: &str, router: Ipv4Addr) -> Option<ArpEntry<'_>> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let [host, address, "at", hardware, "on", interface, rest @ ..] = tokens.as_slice() else {
        return None;
    };
    let named = address.strip_prefix('(')?.strip_suffix(')')?;
    let expected_type = |token: &&str| token.starts_with('[') && token.ends_with(']');
    let suffix_known = rest
        .iter()
        .all(|token| ARP_FLAGS.contains(token) || expected_type(token));
    if *host != "?" || Ipv4Addr::from_str(named).ok()? != router || !suffix_known {
        return None;
    }
    if !is_interface_name(interface) {
        return None;
    }
    if *hardware == "(incomplete)" {
        return Some(ArpEntry::Incomplete);
    }
    Some(ArpEntry::Complete {
        interface,
        hardware: parse_hardware_address(hardware)?,
    })
}

/// the router's hardware address on `interface`, from `arp -n <router>`. Every line
/// must be an entry for `router`; entries on other interfaces are ignored, and
/// the address is refused when the entry is incomplete or missing, or the entries on
/// `interface` disagree.
#[must_use]
pub fn parse_arp(output: &str, router: Ipv4Addr, interface: &str) -> Option<HardwareAddress> {
    let mut found: Option<HardwareAddress> = None;
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        match parse_arp_line(line, router)? {
            ArpEntry::Incomplete => {}
            ArpEntry::Complete {
                interface: on,
                hardware,
            } if on == interface => {
                if found.is_some_and(|earlier| earlier != hardware) {
                    return None;
                }
                found = Some(hardware);
            }
            ArpEntry::Complete { .. } => {}
        }
    }
    found
}

/// how to reach `route` and `arp`.
#[derive(Debug, Clone)]
pub struct NetworkTools {
    pub route: OsString,
    pub arp: OsString,
    pub environment: Vec<(OsString, OsString)>,
    /// the bound of each call.
    pub budget: Duration,
}

impl Default for NetworkTools {
    fn default() -> Self {
        Self {
            route: OsString::from(ROUTE_PROGRAM),
            arp: OsString::from(ARP_PROGRAM),
            environment: Vec::new(),
            budget: PROBE_BUDGET,
        }
    }
}

impl NetworkTools {
    /// `route` and `arp` as this process runs them: the `MARSHAL_TEST_ROUTE` and
    /// `MARSHAL_TEST_ARP` programs in a debug build, else the system's.
    #[must_use]
    pub fn from_environment() -> Self {
        let named = |variable: &str, release: &str| {
            resolve_program(std::env::var_os(variable).as_deref(), release)
        };
        Self {
            route: named(TEST_ROUTE_VARIABLE, ROUTE_PROGRAM),
            arp: named(TEST_ARP_VARIABLE, ARP_PROGRAM),
            ..Self::default()
        }
    }

    /// reads the primary network. `route` runs first; `arp` runs only for a router,
    /// and a failure of it only leaves the hardware address unread.
    pub async fn read(&self) -> NetworkReading {
        let route_arguments = ["-n", "get", "default"].map(str::to_string);
        let entry = match run_bounded(
            &self.route,
            &self.environment,
            &route_arguments,
            self.budget,
        )
        .await
        {
            Ok(answer) if answer.code == Some(0) => parse_route(&answer.stdout),
            _ => None,
        };
        let Some(entry) = entry else {
            return NetworkReading::NoDefaultRoute;
        };
        let hardware = match entry.gateway {
            Some(router) => self.read_hardware(router, &entry.interface).await,
            None => None,
        };
        NetworkReading::Primary(PrimaryNetwork {
            interface: entry.interface,
            router: entry.gateway,
            hardware,
        })
    }

    async fn read_hardware(&self, router: Ipv4Addr, interface: &str) -> Option<HardwareAddress> {
        let arguments = ["-n".to_string(), router.to_string()];
        let answer = run_bounded(&self.arp, &self.environment, &arguments, self.budget)
            .await
            .ok()?;
        if answer.code != Some(0) {
            return None;
        }
        parse_arp(&answer.stdout, router, interface)
    }
}
