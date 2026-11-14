//! what a new configuration does to the pools of the one in use, under the rules of
//! `docs/operations/cli.md`: pools are matched by name; a pool whose only change is
//! its runner count gains or loses its highest-numbered slots; any other change of a
//! pool, of the image or of the budget restarts it. a pool that lists no labels
//! carries the routing label, so a new routing label restarts those pools only.

use serde::Serialize;

use crate::validate::{Entry, Plan};

/// what happens to one pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Unchanged,
    Added,
    Removed,
    RunnersAdded,
    RunnersRemoved,
    Restarted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Effect {
    pub pool: String,
    pub change: Change,
    /// the runners the pool has in the configuration in use.
    pub runners_before: u32,
    /// the runners it has in the new one.
    pub runners_after: u32,
}

impl Effect {
    /// whether the change stops runners, which fails the jobs on them.
    #[must_use]
    pub fn stops_runners(&self) -> bool {
        match self.change {
            Change::Removed | Change::Restarted => self.runners_before > 0,
            Change::RunnersRemoved => true,
            Change::Unchanged | Change::Added | Change::RunnersAdded => false,
        }
    }
}

/// a pool's settings apart from where it sits and how many runners it has.
fn settings(entry: &Entry) -> Entry {
    Entry {
        index: 0,
        path: String::new(),
        slots: 0,
        ..entry.clone()
    }
}

/// the effect on each pool of moving from `in_use`, if a configuration is in use, to
/// `next`: the pools of `next` in its order, then the pools it no longer has.
#[must_use]
pub fn effects(in_use: Option<&Plan>, next: &Plan) -> Vec<Effect> {
    let everything = in_use
        .is_some_and(|in_use| in_use.image_name != next.image_name || in_use.budget != next.budget);
    let held =
        |name: &str| in_use.and_then(|plan| plan.entries.iter().find(|entry| entry.name == name));
    let mut effects: Vec<Effect> = next
        .entries
        .iter()
        .map(|entry| {
            let before = held(&entry.name);
            let runners_before = before.map_or(0, |before| before.slots);
            let change = match before {
                None => Change::Added,
                Some(before) if everything || settings(before) != settings(entry) => {
                    Change::Restarted
                }
                Some(before) if entry.slots > before.slots => Change::RunnersAdded,
                Some(before) if entry.slots < before.slots => Change::RunnersRemoved,
                Some(_) => Change::Unchanged,
            };
            Effect {
                pool: entry.name.clone(),
                change,
                runners_before,
                runners_after: entry.slots,
            }
        })
        .collect();
    if let Some(in_use) = in_use {
        effects.extend(
            in_use
                .entries
                .iter()
                .filter(|entry| !next.entries.iter().any(|kept| kept.name == entry.name))
                .map(|entry| Effect {
                    pool: entry.name.clone(),
                    change: Change::Removed,
                    runners_before: entry.slots,
                    runners_after: 0,
                }),
        );
    }
    effects
}

#[cfg(test)]
mod tests {
    use crate::profile::Profile;
    use crate::validate::validate_toml;

    use super::*;

    fn plan(text: &str) -> Plan {
        let routing = if text.contains("routing_label") {
            ""
        } else {
            "routing_label = \"axpc\"\n"
        };
        let text = format!("schema_version = 1\nimage_name = \"runner:1\"\n{routing}{text}");
        validate_toml(text.as_bytes(), Profile::Axpc, None)
            .outcome
            .unwrap()
    }

    fn pool(name: &str, runners: u32, extra: &str) -> String {
        format!(
            "[[pool]]\nname = \"{name}\"\nrepository = \"o/{name}\"\nrunners = {runners}\n{extra}\n"
        )
    }

    fn changes(before: &Plan, after: &Plan) -> Vec<(String, Change, bool)> {
        effects(Some(before), after)
            .into_iter()
            .map(|effect| {
                let stops = effect.stops_runners();
                (effect.pool, effect.change, stops)
            })
            .collect()
    }

    fn row(pool: &str, change: Change, stops: bool) -> (String, Change, bool) {
        (pool.to_string(), change, stops)
    }

    #[test]
    fn each_row_of_the_table_has_its_effect() {
        let before = plan(&format!(
            "{}{}{}{}{}",
            pool("same", 1, ""),
            pool("more", 1, ""),
            pool("fewer", 3, ""),
            pool("gone", 2, ""),
            pool("relabel", 1, "labels = [\"a\"]"),
        ));
        let after = plan(&format!(
            "{}{}{}{}{}",
            pool("same", 1, ""),
            pool("more", 2, ""),
            pool("fewer", 2, ""),
            pool("relabel", 1, "labels = [\"b\"]"),
            pool("new", 1, ""),
        ));
        assert_eq!(
            changes(&before, &after),
            [
                row("same", Change::Unchanged, false),
                row("more", Change::RunnersAdded, false),
                row("fewer", Change::RunnersRemoved, true),
                row("relabel", Change::Restarted, true),
                row("new", Change::Added, false),
                row("gone", Change::Removed, true),
            ]
        );
    }

    #[test]
    fn a_renamed_pool_is_removed_and_added() {
        let before = plan(&pool("old", 1, ""));
        let after = plan(&pool("young", 1, "").replace("o/young", "o/old"));
        assert_eq!(
            changes(&before, &after),
            [
                row("young", Change::Added, false),
                row("old", Change::Removed, true)
            ]
        );
    }

    #[test]
    fn a_routing_label_restarts_only_the_pools_without_labels() {
        let pools = format!(
            "{}{}",
            pool("plain", 1, ""),
            pool("labelled", 1, "labels = [\"x\"]")
        );
        let before = plan(&format!("routing_label = \"axpc\"\n{pools}"));
        let after = plan(&format!("routing_label = \"axpc-2\"\n{pools}"));
        assert_eq!(
            changes(&before, &after),
            [
                row("plain", Change::Restarted, true),
                row("labelled", Change::Unchanged, false)
            ]
        );
    }

    #[test]
    fn a_new_image_restarts_every_pool() {
        let pools = format!("{}{}", pool("a", 1, ""), pool("b", 2, "labels = [\"x\"]"));
        let before = plan(&pools);
        let after = Plan {
            image_name: Some("runner:2".to_string()),
            ..plan(&pools)
        };
        assert_eq!(
            changes(&before, &after),
            [
                row("a", Change::Restarted, true),
                row("b", Change::Restarted, true)
            ]
        );
    }

    #[test]
    fn with_nothing_in_use_every_pool_is_added() {
        let after = plan(&pool("a", 2, ""));
        let effects = effects(None, &after);
        assert_eq!(
            effects,
            [Effect {
                pool: "a".to_string(),
                change: Change::Added,
                runners_before: 0,
                runners_after: 2
            }]
        );
    }
}
