//! the agent's tokens: every pool's registration reads its target's token from
//! the store, and a request changes it; a token file is never read, and no text
//! this module makes holds a token.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::runtime::log::Log;
use crate::runtime::secret::Secret;
use crate::runtime::token::TokenSource;
use crate::token_store::{StoreError, TokenKey, TokenStore};
use crate::validate::PoolTarget;

/// why a pool has no token to start with; the snapshot and the log show it.
pub const NO_TOKEN: &str = "no token";

/// what the agent says when it has no store at all, as on macOS and Linux until a
/// store is added behind the interface.
pub const NO_TOKEN_STORE: &str = "no token store";

/// how the reason of a pool skipped because the token store failed begins.
pub const STORE_FAILED: &str = "the token store failed";

/// why a pool has no token to start with although this process knows the token is
/// stored: the store answered "no entry" on the read and on each re-read.
pub const STORED_UNREADABLE: &str = "the token is stored but could not be read";

/// how often a read that may only have missed is repeated before it is believed.
pub(crate) const REREADS: u32 = 2;

/// how long the agent waits before each such re-read.
pub(crate) const REREAD_PAUSE: Duration = Duration::from_millis(100);

/// what this process has learned about a token. A key with no entry is unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Learned {
    /// this process saved it, or an earlier read found it.
    Stored,
    /// a read and its re-reads found it missing, or it was removed.
    Absent,
}

/// what a read that re-reads a miss came to.
enum Settled {
    Found(Secret),
    /// missing after the re-reads, though this process knows it is stored.
    StoredButMissing,
    Missing,
    Failed(StoreError),
}

/// the agent's view of the token store.
pub struct StoreTokens {
    store: Option<Arc<dyn TokenStore>>,
    /// which keys had a token when the store was last asked.
    known: RefCell<BTreeMap<TokenKey, bool>>,
    errors: RefCell<BTreeMap<TokenKey, StoreError>>,
    /// what this process has learned about each key; a refresh does not clear it.
    learned: RefCell<BTreeMap<TokenKey, Learned>>,
    /// how long to wait before each re-read; zero in the tests.
    reread_pause: Duration,
    /// waits for the pause; the unit tests record it instead.
    wait: Box<dyn Fn(Duration)>,
    changed: watch::Sender<u64>,
}

impl StoreTokens {
    #[must_use]
    pub fn new(store: Option<Arc<dyn TokenStore>>, reread_pause: Duration) -> Self {
        Self {
            store,
            known: RefCell::new(BTreeMap::new()),
            errors: RefCell::new(BTreeMap::new()),
            learned: RefCell::new(BTreeMap::new()),
            reread_pause,
            wait: Box::new(std::thread::sleep),
            changed: watch::Sender::new(0),
        }
    }

    /// fires when a token was set, so a pool skipped for want of one may start.
    #[must_use]
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// whether the key had a token when the store was last asked.
    #[must_use]
    pub fn has_token(&self, key: &TokenKey) -> bool {
        self.known.borrow().get(key).copied().unwrap_or(false)
    }

    /// asks the store about each key, and keeps the answers for the snapshot;
    /// a store that fails leaves the key without one. A key this process knows is
    /// stored stays one though its reads miss, with a warning.
    pub fn refresh<'a>(&self, keys: impl IntoIterator<Item = &'a TokenKey>, log: &dyn Log) {
        let mut known = self.known.borrow_mut();
        known.clear();
        for key in keys {
            let present = match self.store.as_ref() {
                None => false,
                Some(store) => match self.read_settled(store.as_ref(), key, log) {
                    Settled::Found(_) => true,
                    Settled::StoredButMissing => {
                        log.warn(&format!("{key}: {STORED_UNREADABLE}."));
                        true
                    }
                    Settled::Missing | Settled::Failed(_) => false,
                },
            };
            known.insert(key.clone(), present);
        }
    }

    pub fn set(&self, key: &TokenKey, token: &Secret) -> Result<(), StoreError> {
        let store = self.store.as_ref().ok_or_else(no_store)?;
        if let Err(error) = store.set(key, token) {
            // a failed save can leave no token behind, so the key is unread again
            self.learned.borrow_mut().remove(key);
            return Err(error);
        }
        self.known.borrow_mut().insert(key.clone(), true);
        self.learned
            .borrow_mut()
            .insert(key.clone(), Learned::Stored);
        self.changed.send_modify(|count| *count += 1);
        Ok(())
    }

    pub fn remove(&self, key: &TokenKey) -> Result<(), StoreError> {
        let store = self.store.as_ref().ok_or_else(no_store)?;
        store.remove(key)?;
        self.known.borrow_mut().insert(key.clone(), false);
        self.learned
            .borrow_mut()
            .insert(key.clone(), Learned::Absent);
        Ok(())
    }

    /// the target's token for a registration, or why there is none.
    pub fn read(&self, key: &TokenKey) -> Result<Secret, String> {
        let Some(store) = &self.store else {
            return Err(NO_TOKEN_STORE.to_string());
        };
        match store.get(key) {
            Ok(token) => {
                self.errors.borrow_mut().remove(key);
                token.ok_or_else(|| NO_TOKEN.to_string())
            }
            Err(error) => {
                let reason = format!("{STORE_FAILED}: {}", error.describe());
                self.errors.borrow_mut().insert(key.clone(), error);
                Err(reason)
            }
        }
    }

    /// the key's token for a pool's admission: a miss is read again when the key is
    /// known stored or has not been read yet, and a key known stored that stays
    /// missing is reported as such rather than as having no token.
    fn read_for_admission(&self, key: &TokenKey, log: &dyn Log) -> Result<Secret, String> {
        let Some(store) = &self.store else {
            return Err(NO_TOKEN_STORE.to_string());
        };
        let settled = self.read_settled(store.as_ref(), key, log);
        // the typed failure is kept as a registration's read keeps it
        if let Settled::Failed(error) = &settled {
            self.errors.borrow_mut().insert(key.clone(), error.clone());
        } else {
            self.errors.borrow_mut().remove(key);
        }
        match settled {
            Settled::Found(token) => Ok(token),
            Settled::StoredButMissing => Err(STORED_UNREADABLE.to_string()),
            Settled::Missing => Err(NO_TOKEN.to_string()),
            Settled::Failed(error) => Err(format!("{STORE_FAILED}: {}", error.describe())),
        }
    }

    /// reads the key, and while it misses and this process has no reason to expect it
    /// absent, reads it again up to [`REREADS`] times, a pause apart.
    /// What the process has learned changes only by a read that ends in an answer.
    fn read_settled(&self, store: &dyn TokenStore, key: &TokenKey, log: &dyn Log) -> Settled {
        let before = self.learned.borrow().get(key).copied();
        match store.get(key) {
            Ok(Some(token)) => return self.found(key, token),
            Ok(None) if before == Some(Learned::Absent) => return Settled::Missing,
            Ok(None) => {}
            Err(error) => return Settled::Failed(error),
        }
        for attempt in 1..=REREADS {
            if !self.reread_pause.is_zero() {
                (self.wait)(self.reread_pause);
            }
            match store.get(key) {
                Ok(Some(token)) => {
                    log.info(&format!(
                        "the first read of the token for {key} missed; re-read {attempt} of \
                         {REREADS} found it."
                    ));
                    return self.found(key, token);
                }
                Ok(None) => {}
                Err(error) => return Settled::Failed(error),
            }
        }
        log.info(&format!(
            "the token for {key} was still missing after {REREADS} re-reads."
        ));
        if before == Some(Learned::Stored) {
            return Settled::StoredButMissing;
        }
        self.learned
            .borrow_mut()
            .insert(key.clone(), Learned::Absent);
        Settled::Missing
    }

    fn found(&self, key: &TokenKey, token: Secret) -> Settled {
        self.learned
            .borrow_mut()
            .insert(key.clone(), Learned::Stored);
        Settled::Found(token)
    }

    /// the typed failure from the latest registration read, without another store access.
    pub(crate) fn error(&self, key: &TokenKey) -> Option<StoreError> {
        self.errors.borrow().get(key).cloned()
    }
}

fn no_store() -> StoreError {
    StoreError::new(NO_TOKEN_STORE)
}

impl TokenSource for StoreTokens {
    fn token(&self, target: &PoolTarget, _file: Option<&Path>) -> Result<Secret, String> {
        self.read(&TokenKey::for_pool(target))
    }

    fn admission_token(
        &self,
        target: &PoolTarget,
        _file: Option<&Path>,
        log: &dyn Log,
    ) -> Result<Secret, String> {
        self.read_for_admission(&TokenKey::for_pool(target), log)
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::runtime::fake::MemoryLog;
    use crate::token_store::MemoryTokenStore;

    fn key() -> TokenKey {
        TokenKey::of("Owner", "Repo")
    }

    fn tokens() -> (StoreTokens, Arc<MemoryTokenStore>) {
        let store = Arc::new(MemoryTokenStore::default());
        (StoreTokens::new(Some(store.clone()), Duration::ZERO), store)
    }

    #[test]
    fn a_registration_reads_the_stored_token_by_the_lower_case_key_and_never_the_file() {
        let (tokens, _) = tokens();
        tokens
            .set(&key(), &Secret::new("fake-token".to_string()))
            .unwrap();
        let token = tokens
            .token(
                &PoolTarget::Repository {
                    owner: "OWNER".to_string(),
                    repository: "repo".to_string(),
                },
                Some(Path::new("/no/such/file")),
            )
            .unwrap();
        assert_eq!(token.expose(), "fake-token");
    }

    #[test]
    fn a_missing_token_and_a_missing_store_have_their_own_words() {
        let (tokens, _) = tokens();
        assert_eq!(tokens.read(&key()).unwrap_err(), NO_TOKEN);
        let none = StoreTokens::new(None, Duration::ZERO);
        assert_eq!(none.read(&key()).unwrap_err(), NO_TOKEN_STORE);
        assert!(none.set(&key(), &Secret::new("x".to_string())).is_err());
    }

    #[test]
    fn setting_and_removing_update_what_the_snapshot_knows_but_only_setting_fires_a_change() {
        let (tokens, _) = tokens();
        let mut changes = tokens.changes();
        assert!(!tokens.has_token(&key()));
        tokens
            .set(&key(), &Secret::new("fake".to_string()))
            .unwrap();
        assert!(tokens.has_token(&key()));
        assert!(changes.has_changed().unwrap());
        changes.borrow_and_update();
        tokens.remove(&key()).unwrap();
        assert!(!tokens.has_token(&key()));
        assert!(!changes.has_changed().unwrap());
    }

    #[test]
    fn a_failing_store_is_an_error_whose_text_holds_no_token() {
        let (tokens, store) = tokens();
        tokens
            .set(&key(), &Secret::new("fake-secret".to_string()))
            .unwrap();
        store.fail_with(Some("the vault is locked"));
        let error = tokens.read(&key()).unwrap_err();
        assert!(error.contains("the vault is locked"), "{error}");
        assert!(!error.contains("fake-secret"));
        assert!(
            tokens
                .set(&key(), &Secret::new("fake-secret".to_string()))
                .is_err()
        );
    }

    #[test]
    fn refresh_asks_the_store_for_each_repository() {
        let (tokens, store) = tokens();
        store.set(&key(), &Secret::new("fake".to_string())).unwrap();
        let other = TokenKey::of("o", "other");
        tokens.refresh([&key(), &other], &MemoryLog::default());
        assert!(tokens.has_token(&key()));
        assert!(!tokens.has_token(&other));
    }

    const HIT: bool = true;
    const MISS: bool = false;

    fn secret() -> Secret {
        Secret::new("fake-token".to_string())
    }

    /// what an admission read of `key` comes to, with its log.
    fn admission(tokens: &StoreTokens, key: &TokenKey) -> (Result<Secret, String>, MemoryLog) {
        let log = MemoryLog::default();
        (tokens.read_for_admission(key, &log), log)
    }

    #[test]
    fn the_re_reads_are_two_and_a_tenth_of_a_second_apart() {
        assert_eq!(REREADS, 2);
        assert_eq!(REREAD_PAUSE, Duration::from_millis(100));
    }

    #[test]
    fn each_re_read_waits_for_the_pause_first() {
        let store = Arc::new(MemoryTokenStore::default());
        let mut tokens = StoreTokens::new(Some(store.clone()), REREAD_PAUSE);
        let waits = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&waits);
        tokens.wait = Box::new(move |pause| recorded.borrow_mut().push(pause));
        tokens.set(&key(), &secret()).unwrap();
        store.script_reads(&key(), &[MISS, MISS, HIT]);
        let (result, log) = admission(&tokens, &key());
        assert!(result.is_ok());
        assert_eq!(*waits.borrow(), [REREAD_PAUSE, REREAD_PAUSE]);
        assert_eq!(store.reads_of(&key()), 3);
        assert_eq!(
            log.lines(),
            ["INFO: the first read of the token for owner/repo missed; re-read 2 of 2 found it."]
        );
    }

    #[test]
    fn a_token_known_stored_is_read_again_and_reported_unreadable_when_it_stays_missing() {
        let (tokens, store) = tokens();
        tokens.set(&key(), &secret()).unwrap();
        store.script_reads(&key(), &[MISS, MISS, MISS]);
        let (result, log) = admission(&tokens, &key());
        assert_eq!(result.unwrap_err(), STORED_UNREADABLE);
        assert_eq!(store.reads_of(&key()), 3);
        assert_eq!(
            log.lines(),
            ["INFO: the token for owner/repo was still missing after 2 re-reads."]
        );
        // it is still known stored, so the next read is read again and finds it
        store.script_reads(&key(), &[MISS, HIT]);
        assert!(admission(&tokens, &key()).0.is_ok());
        assert_eq!(store.reads_of(&key()), 5);
    }

    #[test]
    fn a_token_not_read_yet_is_read_again_once_and_then_read_absent() {
        let (tokens, store) = tokens();
        let (result, _) = admission(&tokens, &key());
        assert_eq!(result.unwrap_err(), NO_TOKEN);
        assert_eq!(store.reads_of(&key()), 3);
        let (result, log) = admission(&tokens, &key());
        assert_eq!(result.unwrap_err(), NO_TOKEN);
        assert_eq!(store.reads_of(&key()), 4);
        assert!(log.lines().is_empty());
    }

    #[test]
    fn a_token_read_absent_that_a_read_finds_becomes_known_stored() {
        let (tokens, store) = tokens();
        assert!(admission(&tokens, &key()).0.is_err());
        // another program stores it
        store.set(&key(), &secret()).unwrap();
        assert!(admission(&tokens, &key()).0.is_ok());
        store.script_reads(&key(), &[MISS, MISS, MISS]);
        assert_eq!(admission(&tokens, &key()).0.unwrap_err(), STORED_UNREADABLE);
    }

    #[test]
    fn after_a_removal_through_the_agent_the_absence_is_expected_and_not_read_again() {
        let (tokens, store) = tokens();
        tokens.set(&key(), &secret()).unwrap();
        tokens.remove(&key()).unwrap();
        let (result, _) = admission(&tokens, &key());
        assert_eq!(result.unwrap_err(), NO_TOKEN);
        assert_eq!(store.reads_of(&key()), 1);
    }

    #[test]
    fn a_failed_removal_leaves_what_the_process_learned_unchanged() {
        let (tokens, store) = tokens();
        tokens.set(&key(), &secret()).unwrap();
        store.fail_with(Some("the vault is locked"));
        assert!(tokens.remove(&key()).is_err());
        store.fail_with(None);
        store.script_reads(&key(), &[MISS, MISS, MISS]);
        assert_eq!(admission(&tokens, &key()).0.unwrap_err(), STORED_UNREADABLE);
    }

    #[test]
    fn a_failed_save_makes_the_token_unread_so_its_next_read_re_reads_once_and_settles_it() {
        let (tokens, store) = tokens();
        tokens.set(&key(), &secret()).unwrap();
        store.fail_with(Some("the vault is locked"));
        assert!(tokens.set(&key(), &secret()).is_err());
        store.fail_with(None);
        // the failed save left no token behind
        store.remove(&key()).unwrap();
        let before = store.reads_of(&key());
        assert_eq!(admission(&tokens, &key()).0.unwrap_err(), NO_TOKEN);
        assert_eq!(store.reads_of(&key()) - before, 3);
        // settled: the next read is a single one
        assert!(admission(&tokens, &key()).0.is_err());
        assert_eq!(store.reads_of(&key()) - before, 4);
    }

    #[test]
    fn a_save_makes_an_absent_token_known_stored() {
        let (tokens, store) = tokens();
        assert!(admission(&tokens, &key()).0.is_err());
        tokens.set(&key(), &secret()).unwrap();
        store.script_reads(&key(), &[MISS, MISS, MISS]);
        assert_eq!(admission(&tokens, &key()).0.unwrap_err(), STORED_UNREADABLE);
    }

    #[test]
    fn a_store_error_is_not_read_again_and_leaves_what_the_process_learned_unchanged() {
        let (stored, store) = tokens();
        stored.set(&key(), &secret()).unwrap();
        store.fail_with(Some("the vault is locked"));
        let before = store.reads_of(&key());
        let (result, log) = admission(&stored, &key());
        let error = result.unwrap_err();
        assert!(error.starts_with(STORE_FAILED), "{error}");
        assert_eq!(store.reads_of(&key()) - before, 1);
        assert!(log.lines().is_empty());
        store.fail_with(None);
        store.script_reads(&key(), &[MISS, MISS, MISS]);
        assert_eq!(admission(&stored, &key()).0.unwrap_err(), STORED_UNREADABLE);
        // an error does not settle a key not read yet, whose first answer still re-reads
        let (fresh, store) = tokens();
        store.fail_with(Some("the vault is locked"));
        assert!(admission(&fresh, &key()).0.is_err());
        store.fail_with(None);
        assert_eq!(admission(&fresh, &key()).0.unwrap_err(), NO_TOKEN);
        assert_eq!(store.reads_of(&key()), 4);
    }

    #[test]
    fn an_admission_read_keeps_the_typed_store_failure_as_a_registrations_read_does() {
        let (tokens, store) = tokens();
        tokens.set(&key(), &secret()).unwrap();
        store.fail_with(Some("the vault is locked"));
        assert!(admission(&tokens, &key()).0.is_err());
        assert_eq!(
            tokens.error(&key()).unwrap().to_string(),
            "the vault is locked"
        );
        store.fail_with(None);
        assert!(admission(&tokens, &key()).0.is_ok());
        assert!(tokens.error(&key()).is_none());
        store.fail_with(Some("the vault is locked"));
        assert!(admission(&tokens, &key()).0.is_err());
        store.fail_with(None);
        store.script_reads(&key(), &[MISS, MISS, MISS]);
        assert_eq!(admission(&tokens, &key()).0.unwrap_err(), STORED_UNREADABLE);
        assert!(tokens.error(&key()).is_none());
    }

    #[test]
    fn a_registrations_read_is_never_read_again_and_learns_nothing() {
        let (tokens, store) = tokens();
        store.set(&key(), &secret()).unwrap();
        store.script_reads(&key(), &[MISS]);
        assert_eq!(tokens.read(&key()).unwrap_err(), NO_TOKEN);
        assert_eq!(store.reads_of(&key()), 1);
        // it found the token and learned nothing, so the key is still unread
        assert!(tokens.read(&key()).is_ok());
        store.script_reads(&key(), &[MISS, MISS, MISS]);
        let before = store.reads_of(&key());
        assert_eq!(admission(&tokens, &key()).0.unwrap_err(), NO_TOKEN);
        assert_eq!(store.reads_of(&key()) - before, 3);
    }

    #[test]
    fn a_presence_check_answers_stored_for_a_token_that_stays_missing_once_known_stored() {
        let (tokens, store) = tokens();
        store.set(&key(), &secret()).unwrap();
        let log = MemoryLog::default();
        tokens.refresh([&key()], &log);
        assert!(tokens.has_token(&key()));
        store.script_reads(&key(), &[MISS, MISS, MISS]);
        tokens.refresh([&key()], &log);
        assert!(tokens.has_token(&key()));
        assert!(
            log.lines().contains(
                &"WARN: owner/repo: the token is stored but could not be read.".to_string()
            ),
            "{:?}",
            log.lines()
        );
        // what a refresh learned survives the next one's rebuild of the answers
        store.script_reads(&key(), &[MISS, MISS, MISS]);
        assert_eq!(admission(&tokens, &key()).0.unwrap_err(), STORED_UNREADABLE);
    }

    #[test]
    fn a_presence_check_answers_not_stored_for_a_missing_or_failing_token() {
        let (tokens, store) = tokens();
        let log = MemoryLog::default();
        tokens.refresh([&key()], &log);
        assert!(!tokens.has_token(&key()));
        assert_eq!(store.reads_of(&key()), 3);
        store.set(&key(), &secret()).unwrap();
        store.fail_with(Some("the vault is locked"));
        tokens.refresh([&key()], &log);
        assert!(!tokens.has_token(&key()));
        assert!(log.lines().iter().all(|line| !line.starts_with("WARN")));
    }
}
