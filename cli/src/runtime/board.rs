//! what each pool and slot is doing now, and what the latest image check found,
//! for `status`; a state names pools, slots, containers, images and reasons, never
//! a token or a JIT configuration.

use std::cell::RefCell;
use std::fmt;
use std::time::SystemTime;

use crate::image::check::{ImageCheck, status_fields};
use crate::image::macos_base::MACOS_BASE;
use crate::image::macos_golden;
use crate::runtime::backend::HostWait;

/// where a pool is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolState {
    /// waiting for the backend's engine, named as `engine_name` gives it, to answer.
    WaitingForEngine(&'static str),
    /// its stale containers are being removed and its volumes prepared.
    Preparing,
    Running,
    Skipped(String),
}

impl fmt::Display for PoolState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WaitingForEngine(engine) => write!(f, "waiting for {engine}"),
            Self::Preparing => f.write_str("preparing"),
            Self::Running => f.write_str("running"),
            Self::Skipped(reason) => write!(f, "skipped: {reason}"),
        }
    }
}

/// where a slot is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotState {
    /// preparing its host, probing its CPUs and asking GitHub for a registration.
    Registering,
    /// waiting to try again, for this reason (scrubbed, and with the log's masks applied).
    BackingOff(String),
    /// boots nothing because a host prerequisite refuses, for this reason, and
    /// retries on the backoff schedule.
    Held(String),
    /// preparing its host, which waits for this.
    Waiting(HostWait),
    /// running the job container of this name.
    Running(String),
    /// registers nothing while the agent drains for an update.
    Drained,
    /// being stopped for good, for this reason, such as an edit of the configuration
    /// that removes the slot.
    Leaving(String),
}

/// how `status` and the snapshot's reason say a slot waits for the macOS VM limit.
pub const VM_LIMIT_WAIT: &str = "waiting for the macOS VM limit";

impl fmt::Display for SlotState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registering => f.write_str("registering"),
            Self::BackingOff(_) => f.write_str("backing off"),
            Self::Held(reason) => write!(f, "held: {reason}"),
            Self::Waiting(HostWait::VmLimit) => f.write_str(VM_LIMIT_WAIT),
            Self::Waiting(HostWait::VmDeletion(vm)) => {
                write!(f, "waiting for VM {vm} to be deleted")
            }
            Self::Running(name) => write!(f, "running container {name}"),
            Self::Drained => f.write_str("drained for an update"),
            Self::Leaving(reason) => write!(f, "stopping: {reason}"),
        }
    }
}

#[derive(Debug)]
struct ImageRecord {
    name: String,
    check: Option<ImageCheck>,
}

/// the pools in the order they were first reported, each with its slots, and the
/// image they run.
#[derive(Debug, Default)]
pub struct Board {
    pools: RefCell<Vec<Row<PoolState>>>,
    slots: RefCell<Vec<Row<SlotState>>>,
    image: RefCell<Option<ImageRecord>>,
    /// the newest golden macOS VM's name, `Some(None)` when there is none, or `None`
    /// before a check or with no macOS pool.
    macos_golden: RefCell<Option<Option<String>>>,
    /// the counted leftover job VMs of pools that are no longer configured, from the
    /// latest count.
    vm_leftovers: RefCell<Vec<String>>,
}

/// a pool or slot, its state, and when it entered that state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row<T> {
    pub name: String,
    pub state: T,
    pub since: SystemTime,
}

fn upsert<T: PartialEq>(entries: &RefCell<Vec<Row<T>>>, name: String, state: T) {
    let mut entries = entries.borrow_mut();
    match entries.iter_mut().find(|row| row.name == name) {
        Some(row) if row.state == state => {}
        Some(row) => {
            row.state = state;
            row.since = SystemTime::now();
        }
        None => entries.push(Row {
            name,
            state,
            since: SystemTime::now(),
        }),
    }
}

impl Board {
    /// records a pool's state, under the name a `status` line shows.
    pub fn set_pool(&self, pool: &str, state: PoolState) {
        upsert(&self.pools, pool.to_string(), state);
    }

    /// records a slot's state, under the name a `status` line shows.
    pub fn set_slot(&self, slot: &str, state: SlotState) {
        // a slot that is leaving stays so while its worker winds down
        let leaving = self
            .slots
            .borrow()
            .iter()
            .any(|row| row.name == slot && matches!(row.state, SlotState::Leaving(_)));
        if !leaving || matches!(state, SlotState::Leaving(_)) {
            upsert(&self.slots, slot.to_string(), state);
        }
    }

    /// forgets one slot, which no longer runs.
    pub fn remove_slot(&self, slot: &str) {
        self.slots.borrow_mut().retain(|row| row.name != slot);
    }

    /// forgets one pool, which no longer runs.
    pub fn remove_pool(&self, pool: &str) {
        self.pools.borrow_mut().retain(|row| row.name != pool);
    }

    /// forgets every pool and slot; the latest image check stays.
    pub fn clear(&self) {
        self.pools.borrow_mut().clear();
        self.slots.borrow_mut().clear();
    }

    /// names the image the pools run; a check of another image is forgotten.
    pub fn expect_image(&self, image: &str) {
        let mut record = self.image.borrow_mut();
        if record.as_ref().is_none_or(|held| held.name != image) {
            *record = Some(ImageRecord {
                name: image.to_string(),
                check: None,
            });
        }
    }

    /// records the counted leftover job VMs of unconfigured pools the latest count
    /// found.
    pub fn record_vm_leftovers(&self, names: Vec<String>) {
        self.vm_leftovers.replace(names);
    }

    /// the counted leftover job VMs of unconfigured pools, which `start` does not
    /// delete.
    #[must_use]
    pub fn vm_leftovers(&self) -> Vec<String> {
        self.vm_leftovers.borrow().clone()
    }

    /// forgets the golden macOS VM and the leftover job VMs, for a run with no macOS
    /// pool.
    pub fn forget_macos(&self) {
        self.macos_golden.replace(None);
        self.vm_leftovers.borrow_mut().clear();
    }

    /// records the newest golden macOS VM a check found, or that it found none.
    pub fn record_macos_golden(&self, golden: Option<String>) {
        self.macos_golden.replace(Some(golden));
    }

    /// the newest golden macOS VM the latest check found, as
    /// [`record_macos_golden`](Board::record_macos_golden) took it.
    #[must_use]
    pub fn macos_golden(&self) -> Option<Option<String>> {
        self.macos_golden.borrow().clone()
    }

    /// records what a check of `image` found; an unavailable Docker said nothing
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

    /// every pool with its state and since when, in the order first reported.
    #[must_use]
    pub fn pool_rows(&self) -> Vec<Row<PoolState>> {
        self.pools.borrow().clone()
    }

    /// every slot with its state and since when, in the order first reported.
    #[must_use]
    pub fn slot_rows(&self) -> Vec<Row<SlotState>> {
        self.slots.borrow().clone()
    }

    /// the image the pools run and its latest check, or `None` before one is named.
    #[must_use]
    pub fn image_record(&self) -> Option<(String, Option<ImageCheck>)> {
        self.image
            .borrow()
            .as_ref()
            .map(|record| (record.name.clone(), record.check.clone()))
    }

    /// the `status` lines of the image, its ages counted to `now`, none before an
    /// image is named, then those of the golden macOS VM once a check found it.
    #[must_use]
    pub fn image_fields(&self, now: SystemTime) -> Vec<(&'static str, String)> {
        self.image
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |record| {
                status_fields(&record.name, record.check.as_ref(), now)
            })
            .into_iter()
            .chain(self.macos_golden().into_iter().flat_map(|golden| {
                macos_golden::status_fields(golden.as_deref(), &MACOS_BASE, now)
            }))
            .collect()
    }

    /// one `pool` field per pool, then one `slot` field per slot, each
    /// `<name>: <state>`, then one `leftover-vm` field per counted leftover VM.
    #[must_use]
    pub fn fields(&self) -> Vec<(&'static str, String)> {
        let pools = self
            .pools
            .borrow()
            .iter()
            .map(|row| ("pool", format!("{}: {}", row.name, row.state)))
            .collect::<Vec<_>>();
        let slots = self
            .slots
            .borrow()
            .iter()
            .map(|row| ("slot", format!("{}: {}", row.name, row.state)))
            .collect::<Vec<_>>();
        let leftovers = self
            .vm_leftovers
            .borrow()
            .iter()
            .map(|name| ("leftover-vm", leftover_vm_text(name)))
            .collect::<Vec<_>>();
        pools.into_iter().chain(slots).chain(leftovers).collect()
    }
}

/// how `status` and the log name a counted leftover job VM of an unconfigured pool.
#[must_use]
pub fn leftover_vm_text(name: &str) -> String {
    format!(
        "{name} belongs to a pool that is no longer configured and counts toward the macOS VM \
         limit; delete it with `tart delete {name}`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_pool_and_slot_keeps_its_place_and_its_latest_state() {
        let board = Board::default();
        board.set_pool("alpha", PoolState::WaitingForEngine("Docker"));
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
            PoolState::WaitingForEngine("Docker").to_string(),
            "waiting for Docker"
        );
        assert_eq!(
            PoolState::WaitingForEngine("Tart").to_string(),
            "waiting for Tart"
        );
        assert_eq!(PoolState::Preparing.to_string(), "preparing");
        assert_eq!(
            SlotState::BackingOff("why".to_string()).to_string(),
            "backing off"
        );
        assert_eq!(SlotState::Registering.to_string(), "registering");
        assert_eq!(
            SlotState::Held("Softnet is not set up".to_string()).to_string(),
            "held: Softnet is not set up"
        );
    }
}
