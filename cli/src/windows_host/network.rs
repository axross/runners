//! the primary network on Windows: the interface the default route leaves through,
//! its IPv4 gateway and the gateway's hardware address, read with `netdev`. An
//! interface is named by its kind and index, such as `Wi-Fi 12`, never by its alias,
//! which a user may have named after a person or a place.

use std::net::Ipv4Addr;

#[cfg(windows)]
use crate::macos::network::NetworkReading;
use crate::macos::network::{HardwareAddress, PrimaryNetwork};

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
/// `SendARP` may send an ARP request for the gateway, so the caller bounds it on a
/// blocking thread.
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

#[cfg(test)]
mod tests {
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
}
