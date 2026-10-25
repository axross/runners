//! What each pool and slot is doing now, and what the latest image check found,
//! for `status`.
//!
//! Pool preparation, the image checks and the slot loops write their state here as
//! they go, and the instance reads it when a client asks. A state names pools,
//! slots, containers, images and reasons, never a token or a JIT configuration.

use std::cell::RefCell;
use std::fmt;
use std::time::SystemTime;

use crate::image::check::{ImageCheck, status_fields};

/// Where a pool is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolState {
    WaitingForDocker,
    /// Its stale containers are being removed and its volumes prepared.
    Preparing,
    Running,
    Skipped(String),
}

impl fmt::Display for PoolState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WaitingForDocker => f.write_str("waiting for Docker"),
            Self::Preparing => f.write_str("preparing"),
            Self::Running => f.write_str("running"),
            Self::Skipped(reason) => write!(f, "skipped: {reason}"),
        }
    }
}

/// Where a slot is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotState {
    /// Probing its CPUs and asking GitHub for a registration.
    Registering,
    BackingOff,
    /// Running the job container of this name.
    Running(String),
}

impl fmt::Display for SlotState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registering => f.write_str("registering"),
            Self::BackingOff => f.write_str("backing off"),
            Self::Running(name) => write!(f, "running container {name}"),
        }
    }
}

/// The configured image, and what its latest check found, if one answered.
#[derive(Debug)]
struct ImageRecord {
    name: String,
    check: Option<ImageCheck>,
}

/// The pools in the order they were first reported, each with its slots, and the
/// image they run.
#[derive(Debug, Default)]
pub struct Board {
    pools: RefCell<Vec<(String, PoolState)>>,
    slots: RefCell<Vec<(String, SlotState)>>,
    image: RefCell<Option<ImageRecord>>,
}

fn upsert<T>(entries: &RefCell<Vec<(String, T)>>, key: String, value: T) {
    let mut entries = entries.borrow_mut();
    match entries.iter_mut().find(|(held, _)| *held == key) {
        Some(entry) => entry.1 = value,
        None => entries.push((key, value)),
    }
}

impl Board {
    /// Records a pool's state; `pool` names it as a `status` line shows it.
    pub fn set_pool(&self, pool: &str, state: PoolState) {
        upsert(&self.pools, pool.to_string(), state);
    }

    /// Records a slot's state; `slot` names it as a `status` line shows it.
    pub fn set_slot(&self, slot: &str, state: SlotState) {
        upsert(&self.slots, slot.to_string(), state);
    }

    /// Forgets every pool and slot, as when the runtime core ends. The image check
    /// stays, as the latest one Docker answered.
    pub fn clear(&self) {
        self.pools.borrow_mut().clear();
        self.slots.borrow_mut().clear();
    }

    /// Names the image the pools run. A check of another image is forgotten.
    pub fn expect_image(&self, image: &str) {
        let mut record = self.image.borrow_mut();
        if record.as_ref().is_none_or(|held| held.name != image) {
            *record = Some(ImageRecord {
                name: image.to_string(),
                check: None,
            });
        }
    }

    /// Records what a check of `image` found. An unavailable Docker said nothing
    /// about the image, so it leaves the latest answer in place.
    pub fn record_image(&self, image: &str, check: &ImageCheck) {
        self.expect_image(image);
        if matches!(check, ImageCheck::Unavailable(_)) {
            return;
        }
        if let Some(record) = self.image.borrow_mut().as_mut() {
            record.check = Some(check.clone());
        }
    }

    /// The `status` lines of the image, as of its latest check, its ages counted
    /// to `now`; none before an image is named.
    #[must_use]
    pub fn image_fields(&self, now: SystemTime) -> Vec<(&'static str, String)> {
        self.image
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |record| {
                status_fields(&record.name, record.check.as_ref(), now)
            })
    }

    /// One `pool` field per pool and then one `slot` field per slot, each
    /// `<name>: <state>`.
    #[must_use]
    pub fn fields(&self) -> Vec<(&'static str, String)> {
        let pools = self
            .pools
            .borrow()
            .iter()
            .map(|(name, state)| ("pool", format!("{name}: {state}")))
            .collect::<Vec<_>>();
        let slots = self
            .slots
            .borrow()
            .iter()
            .map(|(name, state)| ("slot", format!("{name}: {state}")))
            .collect::<Vec<_>>();
        pools.into_iter().chain(slots).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_pool_and_slot_keeps_its_place_and_its_latest_state() {
        let board = Board::default();
        board.set_pool("alpha", PoolState::WaitingForDocker);
        board.set_pool("beta", PoolState::Skipped("no token".to_string()));
        board.set_slot("alpha slot 1", SlotState::Registering);
        board.set_pool("alpha", PoolState::Running);
        board.set_slot("alpha slot 1", SlotState::Running("alpha-1-x".to_string()));
        assert_eq!(
            board.fields(),
            [
                ("pool", "alpha: running".to_string()),
                ("pool", "beta: skipped: no token".to_string()),
                (
                    "slot",
                    "alpha slot 1: running container alpha-1-x".to_string()
                ),
            ]
        );
        board.clear();
        assert!(board.fields().is_empty());
    }

    #[test]
    fn the_states_read_as_status_prints_them() {
        assert_eq!(
            PoolState::WaitingForDocker.to_string(),
            "waiting for Docker"
        );
        assert_eq!(PoolState::Preparing.to_string(), "preparing");
        assert_eq!(SlotState::BackingOff.to_string(), "backing off");
        assert_eq!(SlotState::Registering.to_string(), "registering");
    }
}
