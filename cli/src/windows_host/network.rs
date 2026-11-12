//! the primary network on Windows: the interface the default route leaves through,
//! its IPv4 gateway and the gateway's hardware address, read with `netdev`. An
//! interface is named by its kind and index, such as `Wi-Fi 12`, never by its alias,
//! which a user may have named after a person or a place.

use std::cell::RefCell;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

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
    outstanding: RefCell<Option<JoinHandle<NetworkReading>>>,
}

impl Bounded {
    #[must_use]
    pub fn new(read: Reader, limit: Duration) -> Self {
        Self {
            read,
            limit,
            outstanding: RefCell::new(None),
        }
    }

    pub async fn read(&self) -> NetworkReading {
        let previous = self.outstanding.borrow_mut().take();
        if let Some(previous) = previous
            && !previous.is_finished()
        {
            *self.outstanding.borrow_mut() = Some(previous);
            return NetworkReading::NoDefaultRoute;
        }
        let read = Arc::clone(&self.read);
        let mut reading = tokio::task::spawn_blocking(move || read());
        match tokio::time::timeout(self.limit, &mut reading).await {
            Ok(Ok(reading)) => reading,
            Ok(Err(_)) => NetworkReading::NoDefaultRoute,
            Err(_) => {
                *self.outstanding.borrow_mut() = Some(reading);
                NetworkReading::NoDefaultRoute
            }
        }
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
            Some(Ipv4Addr::new(192, 168, 1, 1)),
            Some([2, 0, 0, 0, 0, 1]),
        );
        assert_eq!(wifi.interface, "Wi-Fi 12");
        assert_eq!(wifi.router, Some(Ipv4Addr::new(192, 168, 1, 1)));
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
            Some(Ipv4Addr::new(10, 0, 0, 1)),
            Some([0; 6]),
        );
        assert_eq!(network.hardware, None);
    }

    #[test]
    fn the_same_kind_on_another_index_is_another_network() {
        let router = Some(Ipv4Addr::new(192, 168, 1, 1));
        let a = primary(Kind::WiFi, 12, router, Some([2, 0, 0, 0, 0, 1]));
        let b = primary(Kind::WiFi, 13, router, Some([2, 0, 0, 0, 0, 1]));
        assert!(!a.is_same_network_as(&b));
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
            let finished = bounded
                .outstanding
                .borrow()
                .as_ref()
                .is_some_and(JoinHandle::is_finished);
            if finished {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(bounded.read().await, wifi());
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }
}
