//! the repository-scoped scale-set protocol, without admission or replay policy; the only
//! retry is the registration exchange's, whose fresh token is not always usable at once.

mod transport;
mod types;

use std::time::{Duration, SystemTime};

use reqwest::{Method, Url};
use serde::Deserialize;
use serde_json::json;

use crate::runtime::registration::{HttpLimits, RUNNER_GROUP_ID, resolve_base_url};
use crate::runtime::secret::Secret;
use crate::validate::patterns::{is_owner, is_repository, is_scale_set_name};

pub use transport::{Error, ErrorKind, POLL_TIMEOUT};
pub(crate) use types::ADMIN_RENEWAL_MARGIN;
pub use types::{
    Connection, JitRunner, JobEvent, JobEventKind, Message, Runner, ScaleSet, Session, Statistics,
};

const RUNNER_LIST_MAX_PAGES: u32 = 100;
const RUNNER_LIST_PAGE_SIZE: &str = "100";
const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const EXCHANGE_ATTEMPTS: u32 = 3;
const EXCHANGE_RETRY_DELAY: Duration = Duration::from_secs(5);

/// lifecycle requests independent of the window's display queue and budget.
pub struct Client {
    transport: transport::Transport,
    base: Url,
    exchange_retry_delay: Duration,
}

impl Client {
    /// uses registration's TLS setup and debug-only loopback override, with no redirects.
    pub fn new(base_override: Option<&str>, limits: HttpLimits) -> Result<Self, Error> {
        let base = resolve_base_url(base_override).map_err(|_| Error::new(ErrorKind::Setup))?;
        Ok(Self {
            transport: transport::Transport::new(&base, limits)?,
            base: Url::parse(&base).map_err(|_| Error::new(ErrorKind::Setup))?,
            exchange_retry_delay: EXCHANGE_RETRY_DELAY,
        })
    }

    /// shortens the wait between registration exchange attempts, so a test need not wait seconds.
    #[doc(hidden)]
    #[must_use]
    pub fn with_exchange_retry_delay(mut self, delay: Duration) -> Self {
        self.exchange_retry_delay = delay;
        self
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        self.base.as_str().trim_end_matches('/')
    }

    /// exchanges a repository PAT for a new connection; renewing never mutates an existing session.
    pub async fn connect(
        &self,
        owner: &str,
        repository: &str,
        pat: &Secret,
    ) -> Result<Connection, Error> {
        if !is_owner(owner) || !is_repository(repository) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let registration = self
            .transport
            .rest(
                Method::POST,
                self.rest_url(owner, repository, "registration-token")?,
                pat,
                None,
                &[201],
            )
            .await?;
        #[derive(Deserialize)]
        struct Registration {
            token: String,
        }
        let registration: Registration = transport::decode(&registration)?;
        let registration = nonempty_secret(registration.token)?;
        let response = self.exchange(owner, repository, &registration).await?;
        #[derive(Deserialize)]
        struct Exchange {
            url: String,
            token: String,
        }
        let exchange: Exchange = transport::decode(&response)?;
        let admin = nonempty_secret(exchange.token)?;
        let service_url = self.actions_url(&exchange.url, &[pat, &registration, &admin])?;
        let expires_at = types::token_expiry(&admin)?;
        Ok(Connection {
            owner: owner.to_owned(),
            repository: repository.to_owned(),
            pat: Secret::new(pat.expose().to_owned()),
            admin,
            service_url,
            expires_at,
        })
    }

    /// retries a 401 or 403 only here, because GitHub does not always accept a fresh
    /// registration token at once.
    async fn exchange(
        &self,
        owner: &str,
        repository: &str,
        registration: &Secret,
    ) -> Result<Vec<u8>, Error> {
        let url = self
            .base
            .join("actions/runner-registration")
            .map_err(|_| Error::protocol())?;
        let mut attempt = 1;
        loop {
            let body = json!({"url": format!("https://github.com/{owner}/{repository}"), "runner_event": "register"});
            match self
                .transport
                .exchange(url.clone(), registration, body)
                .await
            {
                Err(error) if error.kind == ErrorKind::Rejected && attempt < EXCHANGE_ATTEMPTS => {
                    attempt += 1;
                    tokio::time::sleep(self.exchange_retry_delay).await;
                }
                result => return result,
            }
        }
    }

    /// reuses or creates group 1's set, refusing incompatible labels without modifying the set.
    pub async fn find_or_create(
        &self,
        connection: &Connection,
        name: &str,
    ) -> Result<ScaleSet, Error> {
        if !is_scale_set_name(name) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let mut url = connection.service_url("runnerscalesets")?;
        url.query_pairs_mut()
            .append_pair("runnerGroupId", &RUNNER_GROUP_ID.to_string())
            .append_pair("name", name);
        let response = self
            .transport
            .service(Method::GET, url, &connection.admin, None, &[200])
            .await?;
        #[derive(Deserialize)]
        struct Sets {
            count: usize,
            value: Vec<types::ScaleSetWire>,
        }
        let sets: Sets = transport::decode(&response)?;
        if sets.count != sets.value.len() || sets.count > 1 {
            return Err(Error::protocol());
        }
        let wire = if let Some(set) = sets.value.into_iter().next() {
            set
        } else {
            let response = self.transport.service(
                Method::POST,
                connection.service_url("runnerscalesets")?,
                &connection.admin,
                Some(json!({"name": name, "runnerGroupId": RUNNER_GROUP_ID, "labels": [{"name": name, "type": "System"}], "RunnerSetting": {"disableUpdate": true}})),
                &[200],
            ).await?;
            transport::decode(&response)?
        };
        wire.validate(name)
    }

    pub(crate) async fn get_scale_set(
        &self,
        connection: &Connection,
        id: u64,
        name: &str,
    ) -> Result<ScaleSet, Error> {
        let response = self
            .transport
            .service(
                Method::GET,
                connection.service_url(&format!("runnerscalesets/{id}"))?,
                &connection.admin,
                None,
                &[200],
            )
            .await?;
        let wire: types::ScaleSetWire = transport::decode(&response)?;
        let set = wire.validate(name)?;
        if set.id() != id {
            return Err(Error::new(ErrorKind::OwnershipMismatch));
        }
        Ok(set)
    }

    /// opens a listener on a validated set; its statistics precede the first poll.
    pub async fn create_session(
        &self,
        connection: &Connection,
        scale_set: &ScaleSet,
        owner: &str,
    ) -> Result<Session, Error> {
        let response = self
            .transport
            .service(
                Method::POST,
                connection.service_url(&format!("runnerscalesets/{}/sessions", scale_set.id()))?,
                &connection.admin,
                Some(json!({"ownerName": owner})),
                &[200],
            )
            .await
            .map_err(session_conflict)?;
        self.session(connection, scale_set.id(), &response)
    }

    /// renews queue credentials with PATCH after a queue 401, preserving the session identity.
    pub async fn refresh_session(
        &self,
        connection: &Connection,
        session: &Session,
    ) -> Result<Session, Error> {
        let response = self
            .transport
            .service(
                Method::PATCH,
                connection.service_url(&session.route())?,
                &connection.admin,
                None,
                &[200],
            )
            .await?;
        let refreshed = self.session(connection, session.scale_set_id, &response)?;
        if refreshed.id != session.id {
            return Err(Error::protocol());
        }
        Ok(refreshed)
    }

    /// parses session credentials without allowing an unvalidated queue destination to escape.
    fn session(
        &self,
        connection: &Connection,
        scale_set_id: u64,
        response: &[u8],
    ) -> Result<Session, Error> {
        let wire: types::SessionWire = transport::decode(response)?;
        if !types::is_session_id(&wire.session_id) {
            return Err(Error::protocol());
        }
        let queue_token = nonempty_secret(wire.message_queue_access_token)?;
        let queue_url = self.actions_url(
            &wire.message_queue_url,
            &[&connection.pat, &connection.admin, &queue_token],
        )?;
        Ok(Session {
            id: wire.session_id,
            scale_set_id,
            queue_url,
            queue_token,
            statistics: wire.statistics,
        })
    }

    /// bounds best-effort closure independently; the caller runs it beside runtime teardown.
    pub async fn close_session(
        &self,
        connection: &Connection,
        session: &Session,
    ) -> Result<(), Error> {
        self.transport
            .service_within(
                Method::DELETE,
                connection.service_url(&session.route())?,
                &connection.admin,
                &[204, 404],
                SESSION_CLOSE_TIMEOUT,
            )
            .await
            .map(|_| ())
    }

    /// no message is acknowledged or progress advanced by this call; `None` retains the caller's statistics.
    pub async fn poll(
        &self,
        session: &Session,
        last_processed_message_id: Option<u64>,
        max_capacity: u32,
    ) -> Result<Option<Message>, Error> {
        let mut url = session.queue_url.clone();
        let retained: Vec<_> = url
            .query_pairs()
            .filter(|(key, _)| key != "lastMessageId")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        url.set_query(None);
        if !retained.is_empty() {
            url.query_pairs_mut().extend_pairs(retained);
        }
        if let Some(id) = last_processed_message_id.filter(|id| *id > 0) {
            url.query_pairs_mut()
                .append_pair("lastMessageId", &id.to_string());
        }
        let response = self
            .transport
            .poll(url, &session.queue_token, max_capacity)
            .await?;
        response.map(|body| types::message(&body)).transpose()
    }

    /// returns only acquired request IDs; a failed response is unresolved, not proof of no acquisition.
    pub async fn acquire_jobs(
        &self,
        connection: &Connection,
        session: &Session,
        request_ids: &[u64],
    ) -> Result<Vec<u64>, Error> {
        let response = self
            .transport
            .service(
                Method::POST,
                connection.service_url(&format!(
                    "runnerscalesets/{}/acquirejobs",
                    session.scale_set_id
                ))?,
                &session.queue_token,
                Some(json!(request_ids)),
                &[200],
            )
            .await?;
        #[derive(Deserialize)]
        struct Acquired {
            count: usize,
            value: Vec<u64>,
        }
        let acquired: Acquired = transport::decode(&response)?;
        if acquired.count != acquired.value.len()
            || acquired.value.iter().any(|id| !request_ids.contains(id))
        {
            return Err(Error::protocol());
        }
        Ok(acquired.value)
    }

    /// call only after processing succeeds; an error leaves acknowledgement and progress unresolved.
    pub async fn acknowledge(&self, session: &Session, message_id: u64) -> Result<(), Error> {
        let mut url = session.queue_url.clone();
        let path = format!("{}/{}", url.path().trim_end_matches('/'), message_id);
        url.set_path(&path);
        self.transport
            .service(Method::DELETE, url, &session.queue_token, None, &[204])
            .await
            .map(|_| ())
    }

    /// a freshly read PAT is required even when service credentials remain unexpired.
    pub async fn request_jit(
        &self,
        connection: &Connection,
        scale_set: &ScaleSet,
        name: &str,
        pat: &Secret,
        now: SystemTime,
    ) -> Result<JitRunner, Error> {
        if connection.needs_renewal(pat, now) {
            return Err(Error::new(ErrorKind::ConnectionRenewalRequired));
        }
        let response = self
            .transport
            .service(
                Method::POST,
                connection.service_url(&format!(
                    "runnerscalesets/{}/generatejitconfig",
                    scale_set.id()
                ))?,
                &connection.admin,
                Some(json!({"name": name, "workFolder": "_work"})),
                &[200],
            )
            .await?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Reference {
            id: u64,
            name: String,
            runner_scale_set_id: u64,
        }
        #[derive(Deserialize)]
        struct Jit {
            runner: Reference,
            #[serde(rename = "encodedJITConfig")]
            encoded_jit_config: String,
        }
        let jit: Jit = transport::decode(&response)?;
        if jit.runner.id == 0
            || jit.runner.name != name
            || jit.runner.runner_scale_set_id != scale_set.id()
        {
            return Err(Error::protocol());
        }
        Ok(JitRunner {
            id: jit.runner.id,
            name: jit.runner.name,
            config: nonempty_secret(jit.encoded_jit_config)?,
        })
    }

    /// reads bounded REST pages built locally, never following a server-supplied link.
    pub async fn list_runners(
        &self,
        owner: &str,
        repository: &str,
        pat: &Secret,
    ) -> Result<Vec<Runner>, Error> {
        let mut runners = Vec::new();
        for page in 1..=RUNNER_LIST_MAX_PAGES {
            let mut url = self.rest_url(owner, repository, "")?;
            url.query_pairs_mut()
                .append_pair("per_page", RUNNER_LIST_PAGE_SIZE)
                .append_pair("page", &page.to_string());
            let response = self
                .transport
                .rest(Method::GET, url, pat, None, &[200])
                .await?;
            #[derive(Deserialize)]
            struct List {
                total_count: usize,
                runners: Vec<Runner>,
            }
            let list: List = transport::decode(&response)?;
            let empty = list.runners.is_empty();
            runners.extend(list.runners);
            if runners.len() >= list.total_count {
                return Ok(runners);
            }
            if empty {
                return Err(Error::protocol());
            }
        }
        Err(Error::protocol())
    }

    /// reconciles an uncertain JIT attempt by its exact name, refusing ambiguous ownership.
    pub async fn find_runner(
        &self,
        owner: &str,
        repository: &str,
        exact_name: &str,
        pat: &Secret,
    ) -> Result<Option<Runner>, Error> {
        let mut matching = self
            .list_runners(owner, repository, pat)
            .await?
            .into_iter()
            .filter(|runner| runner.name == exact_name);
        let runner = matching.next();
        if matching.next().is_some() {
            return Err(Error::new(ErrorKind::OwnershipMismatch));
        }
        Ok(runner)
    }

    /// confirms both ID and exact name before deletion; an already absent registration is settled.
    pub async fn delete_runner(
        &self,
        owner: &str,
        repository: &str,
        id: u64,
        exact_name: &str,
        pat: &Secret,
    ) -> Result<(), Error> {
        let url = self.rest_url(owner, repository, &id.to_string())?;
        let response = match self
            .transport
            .rest(Method::GET, url.clone(), pat, None, &[200])
            .await
        {
            Ok(body) => body,
            Err(error) if error.kind == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let runner: Runner = transport::decode(&response)?;
        if runner.id != id || runner.name != exact_name {
            return Err(Error::new(ErrorKind::OwnershipMismatch));
        }
        match self
            .transport
            .rest(Method::DELETE, url, pat, None, &[204])
            .await
        {
            Ok(_) => Ok(()),
            Err(error) if error.kind == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// confines PAT requests to a validated repository and a locally selected runner endpoint.
    fn rest_url(&self, owner: &str, repository: &str, endpoint: &str) -> Result<Url, Error> {
        if !is_owner(owner) || !is_repository(repository) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let suffix = if endpoint.is_empty() {
            String::new()
        } else {
            format!("/{endpoint}")
        };
        self.base
            .join(&format!(
                "repos/{owner}/{repository}/actions/runners{suffix}"
            ))
            .map_err(|_| Error::protocol())
    }

    fn actions_url(&self, text: &str, secrets: &[&Secret]) -> Result<Url, Error> {
        transport::actions_url(text, &self.base, secrets)
    }
}

/// a 409 names another listener only when opening a session; elsewhere it is a service failure.
fn session_conflict(error: Error) -> Error {
    if error.status == Some(409) {
        Error {
            kind: ErrorKind::Conflict,
            ..error
        }
    } else {
        error
    }
}

/// rejects absent credentials without quoting their source bytes.
fn nonempty_secret(text: String) -> Result<Secret, Error> {
    if text.trim().is_empty() {
        Err(Error::protocol())
    } else {
        Ok(Secret::new(text))
    }
}
