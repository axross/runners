//! the texts `status` and the `start` terminal show for a stop the signals caused.
//! A reason names interfaces and never network names or addresses.

/// what a reason calls the new interface when no interface holds the default route.
pub const NO_INTERFACE: &str = "none";

/// the reason of a stop at a sleep.
#[must_use]
pub fn sleep_reason() -> String {
    "the Mac slept, so every runner was stopped".to_string()
}

/// the reason of a stop at a change of the primary network.
#[must_use]
pub fn network_change_reason(old_interface: &str, new_interface: &str) -> String {
    format!(
        "the primary network changed ({old_interface} to {new_interface}), so every runner was stopped"
    )
}
