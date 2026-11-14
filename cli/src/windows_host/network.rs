//! the primary network on Windows: the interface the default route leaves through,
//! its IPv4 gateway and the gateway's hardware address, read with `netdev`. An
//! interface is named by its kind and index, such as `Wi-Fi 12`, never by its alias,
//! which a user may have named after a person or a place.

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::macos::network::{HardwareAddress, NetworkReading, PrimaryNetwork};

/// what the name of an interface says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ethernet,
    WiFi,
    Cellular,
    Vpn,
    Other,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "Ethernet",
            Self::WiFi => "Wi-Fi",
            Self::Cellular => "Cellular",
            Self::Vpn => "VPN",
            Self::Other => "Network",
        }
    }
}

/// the primary network an interface of `kind` and `index` carries. A gateway hardware
/// address of all zeros is one that could not be read.
#[must_use]
pub fn primary(
    kind: Kind,
    index: u32,
    gateway: Option<Ipv4Addr>,
    hardware: Option<[u8; 6]>,
) -> PrimaryNetwork {
    PrimaryNetwork {
        interface: format!("{} {index}", kind.label()),
        router: gateway,
        hardware: hardware
            .filter(|bytes| bytes.iter().any(|&b| b != 0))
            .map(HardwareAddress::new),
    }
}

#[cfg(windows)]
fn kind(interface_type: netdev::interface::types::InterfaceType) -> Kind {
    use netdev::interface::types::InterfaceType as T;
    match interface_type {
        T::Ethernet
        | T::Ethernet3Megabit
        | T::FastEthernetT
        | T::FastEthernetFx
        | T::GigabitEthernet => Kind::Ethernet,
        T::Wireless80211 => Kind::WiFi,
        T::Wwan | T::Wwanpp | T::Wwanpp2 => Kind::Cellular,
        T::Ppp | T::Tunnel | T::ProprietaryVirtual => Kind::Vpn,
        _ => Kind::Other,
    }
}

/// reads the primary network now; a failed read is `NoDefaultRoute`. It blocks, and
/// `SendARP` may send an ARP request for the gateway, so [`Bounded`] runs it.
#[cfg(windows)]
#[must_use]
pub fn read() -> NetworkReading {
    let Ok(interface) = netdev::get_default_interface() else {
        return NetworkReading::NoDefaultRoute;
    };
    let gateway = interface.gateway.as_ref();
    NetworkReading::Primary(primary(
        kind(interface.if_type),
        interface.index,
        gateway.and_then(|device| device.ipv4.first().copied()),
        gateway.map(|device| device.mac_addr.octets()),
    ))
}

/// how long one reading may take before it counts as `NoDefaultRoute`.
pub const READ_LIMIT: Duration = Duration::from_secs(2);

type Reader = Arc<dyn Fn() -> NetworkReading + Send + Sync>;

/// a reader run on a blocking thread, bounded to a limit, with at most one reading
/// outstanding: while one that ran past its limit has not returned, the next reading
/// is `NoDefaultRoute` and starts nothing.
pub struct Bounded {
    read: Reader,
    limit: Duration,
    /// set while a reading runs; the blocking task clears it, so a caller that drops
    /// its wait does not lose track of the reading.
    in_flight: Arc<AtomicBool>,
}

/// clears the in-flight mark when the reading ends, also when the reader panics.
struct Done(Arc<AtomicBool>);

impl Drop for Done {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Bounded {
    #[must_use]
    pub fn new(read: Reader, limit: Duration) -> Self {
        Self {
            read,
            limit,
            in_flight: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn read(&self) -> NetworkReading {
        if self.in_flight.swap(true, Ordering::AcqRel) {
            return NetworkReading::NoDefaultRoute;
        }
        let done = Done(Arc::clone(&self.in_flight));
        let read = Arc::clone(&self.read);
        let reading = tokio::task::spawn_blocking(move || {
            let _done = done;
            read()
        });
        match tokio::time::timeout(self.limit, reading).await {
            Ok(Ok(reading)) => reading,
            Ok(Err(_)) | Err(_) => NetworkReading::NoDefaultRoute,
        }
    }

    /// whether a reading is still running.
    #[cfg(test)]
    fn is_reading(&self) -> bool {
        self.in_flight.load(Ordering::Acquire)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn an_interface_is_named_by_kind_and_index() {
        let wifi = primary(
            Kind::WiFi,
            12,
            Some(Ipv4Addr::new(192, 0, 2, 1)),
            Some([2, 0, 0, 0, 0, 1]),
        );
        assert_eq!(wifi.interface, "Wi-Fi 12");
        assert_eq!(wifi.router, Some(Ipv4Addr::new(192, 0, 2, 1)));
        assert_eq!(
            wifi.hardware,
            Some(HardwareAddress::new([2, 0, 0, 0, 0, 1]))
        );
        assert_eq!(primary(Kind::Vpn, 7, None, None).interface, "VPN 7");
    }

    #[test]
    fn an_all_zero_hardware_address_is_one_that_could_not_be_read() {
        let network = primary(
            Kind::Ethernet,
            3,
            Some(Ipv4Addr::new(198, 51, 100, 1)),
            Some([0; 6]),
        );
        assert_eq!(network.hardware, None);
    }

    #[test]
    fn the_same_kind_on_another_index_is_another_network() {
        let router = Some(Ipv4Addr::new(192, 0, 2, 1));
        let a = primary(Kind::WiFi, 12, router, Some([2, 0, 0, 0, 0, 1]));
        let b = primary(Kind::WiFi, 13, router, Some([2, 0, 0, 0, 0, 1]));
        assert!(!a.is_same_network_as(&b));
    }

    /// the machines that run this test have a default route, so Windows' own answer is
    /// a primary network on a real interface.
    #[cfg(windows)]
    #[test]
    fn windows_reads_the_primary_network() {
        let reading = read();
        assert!(
            matches!(&reading, NetworkReading::Primary(network) if !network.interface.is_empty()),
            "no primary network was read"
        );
    }

    fn wifi() -> NetworkReading {
        NetworkReading::Primary(primary(Kind::WiFi, 12, None, None))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_hung_reading_counts_as_no_route_and_blocks_a_second_until_it_returns() {
        let started = Arc::new(AtomicU32::new(0));
        let (release, released) = mpsc::channel::<()>();
        let released = Mutex::new(released);
        let counter = Arc::clone(&started);
        let bounded = Bounded::new(
            Arc::new(move || {
                // the first reading hangs until the test lets it return
                if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                    let _ = released.lock().unwrap().recv();
                }
                wifi()
            }),
            Duration::from_millis(50),
        );
        assert_eq!(bounded.read().await, NetworkReading::NoDefaultRoute);
        assert_eq!(bounded.read().await, NetworkReading::NoDefaultRoute);
        assert_eq!(started.load(Ordering::SeqCst), 1);
        release.send(()).unwrap();
        for _ in 0..200 {
            if !bounded.is_reading() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(bounded.read().await, wifi());
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_reading_whose_wait_was_dropped_still_blocks_a_second() {
        let started = Arc::new(AtomicU32::new(0));
        let (release, released) = mpsc::channel::<()>();
        let released = Mutex::new(released);
        let counter = Arc::clone(&started);
        let bounded = Bounded::new(
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = released.lock().unwrap().recv();
                wifi()
            }),
            Duration::from_secs(5),
        );
        // the caller gives up long before the limit, as a stop does
        let dropped = tokio::time::timeout(Duration::from_millis(20), bounded.read()).await;
        assert!(dropped.is_err());
        assert_eq!(bounded.read().await, NetworkReading::NoDefaultRoute);
        assert_eq!(started.load(Ordering::SeqCst), 1);
        release.send(()).unwrap();
    }
}
