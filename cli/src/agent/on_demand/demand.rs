use std::collections::BTreeMap;

use crate::scaleset::{JobEventKind, Message, Statistics};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Offer {
    Pending,
    Uncertain { reflected: bool },
    Acquired { reflected: bool, leased: bool },
    Settled,
}

#[derive(Debug, Clone)]
pub(super) struct Lease {
    pub slot: u32,
    pub offer: Option<u64>,
}

/// uncertainty consumes capacity until the same request IDs have been reconciled.
#[derive(Default)]
pub(super) struct Demand {
    pub statistics: Statistics,
    pub leases: BTreeMap<u64, Lease>,
    offers: BTreeMap<u64, Offer>,
    completed_since_statistics: u64,
    completed_before_finish: u64,
    next_lease: u64,
    message: Option<u64>,
}

impl Demand {
    /// session recreation replaces demand, but never releases a still-owned registration.
    pub fn initial(&mut self, statistics: Statistics) {
        self.update_statistics(statistics);
        self.message = None;
        self.offers
            .retain(|_, offer| !matches!(offer, Offer::Settled));
        let mut remaining = statistics
            .total_assigned_jobs
            .saturating_sub(self.leases.len() as u64);
        for offer in self.offers.values_mut() {
            match offer {
                Offer::Acquired { reflected, .. } => *reflected = true,
                Offer::Uncertain { reflected } => {
                    *reflected = remaining > 0;
                    remaining = remaining.saturating_sub(1);
                }
                _ => {}
            }
        }
    }

    /// duplicate delivery may carry fresher statistics; event steps remain idempotent.
    pub fn receive(&mut self, message: &Message) {
        if self.message != Some(message.id) || self.statistics != message.statistics {
            self.update_statistics(message.statistics);
        }
        self.message = Some(message.id);
        for job in &message.jobs {
            match job.kind {
                JobEventKind::Available => {
                    self.offers.entry(job.request_id).or_insert(Offer::Pending);
                }
                JobEventKind::Assigned | JobEventKind::Started | JobEventKind::Completed => {
                    if let Some(Offer::Acquired { reflected, .. }) =
                        self.offers.get_mut(&job.request_id)
                    {
                        *reflected = true;
                    } else {
                        self.offers.insert(job.request_id, Offer::Settled);
                    }
                }
            }
        }
    }

    /// refreshed session statistics settle acquire uncertainty without depending on another event.
    pub fn update_statistics(&mut self, statistics: Statistics) {
        let growth = statistics
            .total_assigned_jobs
            .saturating_sub(self.statistics.total_assigned_jobs);
        let completed = self
            .statistics
            .total_assigned_jobs
            .saturating_sub(statistics.total_assigned_jobs)
            .saturating_sub(self.completed_since_statistics);
        // GitHub can observe completion before the runtime finishes its registration cleanup.
        self.completed_before_finish = self
            .completed_before_finish
            .saturating_add(completed)
            .min(self.leases.len() as u64);
        self.statistics = statistics;
        self.completed_since_statistics = 0;
        let mut reflected = 0;
        for offer in self.offers.values_mut() {
            if reflected >= growth {
                break;
            }
            if let Offer::Acquired {
                reflected: seen, ..
            }
            | Offer::Uncertain { reflected: seen } = offer
                && !*seen
            {
                *seen = true;
                reflected += 1;
            }
        }
    }

    pub fn uncertain(&self) -> usize {
        self.offers
            .values()
            .filter(|offer| matches!(offer, Offer::Uncertain { .. }))
            .count()
    }

    fn acquired_unleased(&self) -> usize {
        self.offers
            .values()
            .filter(|offer| matches!(offer, Offer::Acquired { leased: false, .. }))
            .count()
    }

    pub fn occupied(&self) -> usize {
        self.leases.len() + self.uncertain() + self.acquired_unleased()
    }

    fn desired(&self) -> u64 {
        let unreflected = self
            .offers
            .values()
            .filter(|offer| {
                matches!(
                    offer,
                    Offer::Acquired {
                        reflected: false,
                        ..
                    }
                )
            })
            .count() as u64;
        self.statistics
            .total_assigned_jobs
            .saturating_sub(self.completed_since_statistics)
            .saturating_add(unreflected)
    }

    /// a retry uses the same uncertain IDs even though their capacity is already held.
    pub fn acquire(&mut self, capacity: u32) -> Vec<u64> {
        let retry: Vec<_> = self
            .offers
            .iter()
            .filter_map(|(&id, offer)| matches!(offer, Offer::Uncertain { .. }).then_some(id))
            .take((capacity as usize).saturating_sub(self.leases.len() + self.acquired_unleased()))
            .collect();
        if !retry.is_empty() {
            return retry;
        }
        let assigned_waiting = self
            .desired()
            .saturating_sub(self.leases.len() as u64 + self.acquired_unleased() as u64);
        let free = u64::from(capacity)
            .saturating_sub(self.occupied() as u64)
            .saturating_sub(assigned_waiting);
        let ids: Vec<_> = self
            .offers
            .iter()
            .filter_map(|(&id, offer)| (*offer == Offer::Pending).then_some(id))
            .take(usize::try_from(free).unwrap_or(usize::MAX))
            .collect();
        for id in &ids {
            self.offers
                .insert(*id, Offer::Uncertain { reflected: false });
        }
        ids
    }

    /// omission confirms that the service no longer considers an offer pending.
    pub fn acquired(&mut self, requested: &[u64], acquired: &[u64]) {
        for id in requested {
            let reflected = matches!(
                self.offers.get(id),
                Some(Offer::Uncertain { reflected: true })
            );
            self.offers.insert(
                *id,
                if acquired.contains(id) {
                    Offer::Acquired {
                        reflected,
                        leased: false,
                    }
                } else {
                    Offer::Settled
                },
            );
        }
    }

    pub fn processed(&self) -> bool {
        self.offers
            .values()
            .all(|offer| !matches!(offer, Offer::Pending | Offer::Uncertain { .. }))
    }

    pub fn acknowledged(&mut self) {
        self.offers
            .retain(|_, offer| !matches!(offer, Offer::Settled));
    }

    /// slot reservations include preparation, delayed JIT and incomplete cleanup.
    pub fn reserve(&mut self, slot: u32, capacity: u32) -> Option<u64> {
        if slot == 0
            || slot > capacity
            || self.leases.values().any(|lease| lease.slot == slot)
            || self.leases.len() as u64 >= u64::from(capacity).min(self.desired())
            || self.leases.len() + self.uncertain() >= capacity as usize
        {
            return None;
        }
        let offer = self.offers.iter_mut().find_map(|(&id, offer)| {
            if let Offer::Acquired { leased, .. } = offer
                && !*leased
            {
                *leased = true;
                return Some(id);
            }
            None
        });
        self.next_lease = self.next_lease.checked_add(1)?;
        self.leases.insert(self.next_lease, Lease { slot, offer });
        Some(self.next_lease)
    }

    /// only a completed job consumes retained demand; a failed start remains retryable on 202.
    pub fn finish(&mut self, id: u64, completed: bool) {
        let Some(lease) = self.leases.remove(&id) else {
            return;
        };
        let unreflected = lease
            .offer
            .and_then(|offer| self.offers.get(&offer))
            .is_some_and(|offer| {
                matches!(
                    offer,
                    Offer::Acquired {
                        reflected: false,
                        ..
                    }
                )
            });
        if completed {
            if !unreflected {
                if self.completed_before_finish > 0 {
                    self.completed_before_finish -= 1;
                } else {
                    self.completed_since_statistics =
                        self.completed_since_statistics.saturating_add(1);
                }
            }
            if let Some(offer) = lease.offer {
                self.offers.insert(offer, Offer::Settled);
            }
        } else if let Some(Offer::Acquired { leased, .. }) =
            lease.offer.and_then(|id| self.offers.get_mut(&id))
        {
            *leased = false;
        }
        self.completed_before_finish = self.completed_before_finish.min(self.leases.len() as u64);
    }
}
