//! the configuration methods. `config_read` is answered in any state; `config_edit`
//! and `config_reload` write and apply while the runners run, write only while they
//! are stopped, and are refused during a transition, while an edit applies and while
//! the runtime core waits to restart.

use std::collections::BTreeSet;
use std::hash::{DefaultHasher, Hasher};
use std::io;
use std::path::PathBuf;

use serde_json::{Value as Json, json};

use crate::checkout;
use crate::edit::{ConfigFile, Edit, EditError, rewrite};
use crate::host_config::{Checks, FileStatus, InUse, Loaded, Migration, Origin, StandardChecks};
use crate::runtime::backend::Backend;
use crate::runtime::clock::Clock;
use crate::runtime::registration::Registrar;
use crate::token_store::TokenKey;
use crate::validate::{CheckoutRule, Failure, Plan};

use super::super::api::{ApiError, ApiResult, Method};
use super::super::builder::ImageBuilder;
use super::super::config::{
    Change, EditsError, Effect, Problem, editable, effects, problem, read_edits,
};
use super::super::core::ApplyOrder;
use super::super::snapshot::{ApplyInfo, MAX_PROBLEMS};
use super::super::state::{Applying, DRAINING, Phase};
use super::super::view::{migration_failure_text, millis};
use super::super::wire::{ErrorCode, MAX_AGENT_MESSAGE_BYTES};
use super::{Agent, Reply};

/// the room a `config_read` result leaves for the response around it.
const RESPONSE_ROOM: usize = 1024;

/// a change of the configuration a client asks for.
pub(super) enum Request {
    Edit { revision: String, edits: Vec<Edit> },
    Reload { revision: String },
}

/// what the state lets an edit or a reload do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// the runners run: the change is written and applied.
    Apply,
    /// the runners are stopped: the change is written and used at the next Start all.
    Write,
}

/// the reply of a change on a running agent; dropped unanswered, as when a stop ends
/// the runners while the image is checked, it says the change was not written.
struct Answer(Option<Reply>);

impl Drop for Answer {
    fn drop(&mut self) {
        if let Some(reply) = self.0.take() {
            let _ = reply.send(Err(error(
                ErrorCode::Busy,
                "the runners stopped before the change was written",
            )));
        }
    }
}

/// a change that passed every check, ready to be written.
struct Prepared {
    file: ConfigFile,
    /// `None` for a reload, which writes nothing.
    edits: Option<Vec<Edit>>,
    plan: Plan,
    revision: String,
    effects: Vec<Effect>,
    released: Vec<String>,
}

/// the revision of the file's bytes: a digest compared for equality only, which names
/// no path.
#[must_use]
pub fn revision(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    format!("{:016x}{:08x}", hasher.finish(), bytes.len())
}

fn error(code: ErrorCode, message: impl Into<String>) -> ApiError {
    ApiError::new(code, message)
}

fn with_problems(code: ErrorCode, message: &str, problems: &[String]) -> ApiError {
    ApiError {
        problems: problems
            .iter()
            .take(MAX_PROBLEMS)
            .map(|text| problem(text))
            .collect(),
        ..error(code, message)
    }
}

fn failure_error(failure: &Failure) -> ApiError {
    match failure {
        Failure::Invalid(problems) => with_problems(
            ErrorCode::ConfigInvalid,
            "the configuration breaks the rules",
            problems,
        ),
        Failure::Unreadable(text) => with_problems(
            ErrorCode::ConfigUnusable,
            "config.toml cannot be read as TOML; fix it in a text editor",
            std::slice::from_ref(text),
        ),
    }
}

fn edit_error(failure: EditError) -> ApiError {
    match failure {
        EditError::Changed => error(
            ErrorCode::ConfigChanged,
            "config.toml changed after it was read",
        ),
        EditError::Invalid(failure) => failure_error(&failure),
        EditError::Io { .. } => error(
            ErrorCode::ConfigUnwritable,
            format!("config.toml was not written: {failure}"),
        ),
        EditError::Inapplicable(reason) => error(ErrorCode::InvalidRequest, reason),
        EditError::FileForm(reason) => error(
            ErrorCode::ConfigFileForm,
            format!("config.toml is written in a form the app does not edit: {reason}"),
        ),
    }
}

impl<B: Backend, K: Clock, R: Registrar, D: ImageBuilder> Agent<'_, B, K, R, D> {
    /// answers `config_read`: the file as it is now, which the record of the
    /// configuration in use does not follow.
    pub(super) fn config_read(&self) -> Json {
        let file = match ConfigFile::read(&self.config_path()) {
            Ok(file) => file,
            Err(EditError::Io {
                kind: io::ErrorKind::NotFound,
                ..
            }) => return read_result("", "missing", &[], None),
            Err(failure) => {
                return read_result("", "unreadable", &[failure.to_string()], None);
            }
        };
        let bytes = file.bytes();
        let above = self.above();
        let (status, problems) = match self
            .checks(above.as_deref())
            .check_toml(bytes, self.settings.profile)
            .outcome
        {
            Ok(_) => ("valid", Vec::new()),
            Err(Failure::Invalid(problems)) => ("invalid", problems),
            Err(Failure::Unreadable(text)) => ("unreadable", vec![text]),
        };
        let values = (status != "unreadable")
            .then(|| String::from_utf8(bytes.to_vec()).ok())
            .flatten()
            .and_then(|text| {
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
                editable(&text, self.settings.profile)
            });
        read_result(&revision(bytes), status, &problems, values)
    }

    /// what the state lets an edit or a reload do, or why it is refused.
    pub(super) fn config_mode(&self) -> Result<Mode, ApiError> {
        let inner = self.inner.borrow();
        // a drain for an update takes no method but a stop and a quit
        if *self.drain.borrow() {
            return Err(error(ErrorCode::Refused, DRAINING));
        }
        match &inner.phase {
            Phase::Stopped(_) => Ok(Mode::Write),
            Phase::Running if inner.restart_due.is_some() || self.orders.borrow().is_none() => {
                Err(error(
                    ErrorCode::RuntimeRestarting,
                    "the runtime core is waiting to restart after a failure",
                ))
            }
            Phase::Running if inner.applying.is_some() => Err(error(
                ErrorCode::ConfigApplying,
                "an edit of the configuration is being applied",
            )),
            Phase::Running => Ok(Mode::Apply),
            phase => Err(error(
                ErrorCode::Busy,
                format!("the agent is {}", phase.name()),
            )),
        }
    }

    /// reads the request of `config_edit` or `config_reload`.
    pub(super) fn config_request(&self, method: Method) -> Result<(Request, bool), ApiError> {
        match method {
            Method::ConfigEdit {
                revision,
                edits,
                check,
            } => {
                let edits =
                    read_edits(edits.0, self.settings.profile).map_err(
                        |failure| match failure {
                            EditsError::Invalid(message) => {
                                error(ErrorCode::InvalidRequest, message)
                            }
                            EditsError::Refused(message) => {
                                error(ErrorCode::ConfigRefused, message)
                            }
                        },
                    )?;
                Ok((Request::Edit { revision, edits }, check))
            }
            Method::ConfigReload { revision, check } => Ok((Request::Reload { revision }, check)),
            _ => Err(error(
                ErrorCode::InvalidRequest,
                "not a configuration change",
            )),
        }
    }

    /// checks a change while the runners are stopped, and writes it unless `check`.
    pub(super) fn configure_stopped(&self, request: Request, check: bool) -> ApiResult {
        let prepared = self.prepare(request)?;
        if check {
            return Ok(self.change_result(&prepared, Mode::Write));
        }
        self.commit(prepared, Mode::Write)
    }

    /// checks a change while the runners run, with the image it names when it names
    /// another, and writes and applies it unless `check`; the reply is sent once it is
    /// written and its apply has started.
    pub(super) async fn configure_running(&self, request: Request, check: bool, reply: Reply) {
        let mut answer = Answer(Some(reply));
        let result = self.configure_running_result(request, check).await;
        if let Some(reply) = answer.0.take() {
            let _ = reply.send(result);
        }
    }

    async fn configure_running_result(&self, request: Request, check: bool) -> ApiResult {
        let prepared = self.prepare(request)?;
        let in_use = self
            .inner
            .borrow()
            .plan()
            .and_then(|plan| plan.image_name.clone());
        // the board keeps what the check of the image in use found, so a check that
        // writes nothing leaves it alone
        let mut checked = None;
        if let Some(image) = prepared.plan.image_name.as_deref()
            && in_use.as_deref() != Some(image)
        {
            let found = self.backend.check_image(image).await;
            if let Some(refusal) = found.refusal(image) {
                return Err(error(ErrorCode::ImageUnusable, refusal));
            }
            checked = Some((image.to_string(), found));
        }
        if check {
            return Ok(self.change_result(&prepared, Mode::Apply));
        }
        // the state may have changed while the image was checked
        if self.config_mode()? != Mode::Apply {
            return Err(error(ErrorCode::Busy, "the agent is no longer running"));
        }
        let result = self.commit(prepared, Mode::Apply)?;
        if let Some((image, found)) = checked {
            self.board.record_image(&image, &found);
        }
        Ok(result)
    }

    /// checks a change against the file and the rules the agent loads with.
    fn prepare(&self, request: Request) -> Result<Prepared, ApiError> {
        self.refuse_migration()?;
        let file = ConfigFile::read(&self.config_path()).map_err(|failure| match failure {
            EditError::Io {
                kind: io::ErrorKind::NotFound,
                ..
            } => error(
                ErrorCode::ConfigUnusable,
                format!(
                    "there is no configuration at {}",
                    self.config_path().display()
                ),
            ),
            failure => error(
                ErrorCode::ConfigUnusable,
                format!("config.toml could not be read: {failure}"),
            ),
        })?;
        let (expected, edits) = match request {
            Request::Edit { revision, edits } => (revision, Some(edits)),
            Request::Reload { revision } => (revision, None),
        };
        if revision(file.bytes()) != expected {
            return Err(error(
                ErrorCode::ConfigChanged,
                "config.toml changed after it was read",
            ));
        }
        let profile = self.settings.profile;
        let above = self.above();
        let output = match &edits {
            Some(edits) => {
                rewrite(file.bytes(), profile, above.as_deref(), edits).map_err(edit_error)?
            }
            None => file.bytes().to_vec(),
        };
        let validation = self.checks(above.as_deref()).check_toml(&output, profile);
        let plan = match validation.outcome {
            Ok(plan) => plan,
            Err(failure) => {
                if edits.is_none() {
                    self.keep_in_use(&failure);
                }
                return Err(failure_error(&failure));
            }
        };
        let inner = self.inner.borrow();
        let in_use = inner.plan();
        let named = |plan: &Plan| -> BTreeSet<TokenKey> {
            plan.entries
                .iter()
                .map(|entry| TokenKey::of(&entry.owner, &entry.repository))
                .collect()
        };
        let released = in_use.map_or_else(Vec::new, |in_use| {
            named(in_use)
                .difference(&named(&plan))
                .map(|key| key.as_str().to_string())
                .collect()
        });
        Ok(Prepared {
            effects: effects(in_use, &plan),
            revision: revision(&output),
            released,
            plan,
            edits,
            file,
        })
    }

    /// refuses a change while the migration of `config.json` failed or is pending.
    fn refuse_migration(&self) -> Result<(), ApiError> {
        let inner = self.inner.borrow();
        match inner.loaded.as_ref().map(|loaded| &loaded.migration) {
            Some(Migration::Failed(failure)) => Err(error(
                ErrorCode::ConfigUnusable,
                migration_failure_text(failure),
            )),
            Some(Migration::Pending) => Err(error(
                ErrorCode::ConfigUnusable,
                "Marshal migrates config.json when it next starts: quit Marshal and launch it again",
            )),
            _ => Ok(()),
        }
    }

    /// writes the change, makes it the configuration in use, and starts its apply
    /// while the runners run.
    fn commit(&self, prepared: Prepared, mode: Mode) -> ApiResult {
        if let Some(edits) = &prepared.edits {
            let above = self.above();
            prepared
                .file
                .edit(self.settings.profile, above.as_deref(), edits)
                .map_err(edit_error)?;
        }
        let result = self.change_result(&prepared, mode);
        let previous = self.use_file(prepared.plan.clone());
        self.refresh_tokens(&prepared.plan);
        if mode == Mode::Apply {
            self.start_apply(prepared, previous);
        }
        self.publish();
        Ok(result)
    }

    /// makes `plan`, just read from the file, the configuration in use, and returns
    /// the one it replaces.
    fn use_file(&self, plan: Plan) -> Option<Plan> {
        let mut inner = self.inner.borrow_mut();
        let previous = inner.loaded.take();
        let migration = match previous.as_ref().map(|loaded| &loaded.migration) {
            Some(migrated @ Migration::Migrated { .. }) => migrated.clone(),
            _ => Migration::None,
        };
        inner.loaded = Some(Loaded {
            file: FileStatus::Valid,
            in_use: Some(InUse {
                origin: Origin::File,
                plan,
                read_at: self.clock.now(),
            }),
            migration,
            problems: Vec::new(),
        });
        previous
            .and_then(|loaded| loaded.in_use)
            .map(|in_use| in_use.plan)
    }

    /// records a reload that found the file unusable: the configuration in use stays,
    /// and one read from the file is the last valid one.
    fn keep_in_use(&self, failure: &Failure) {
        let mut inner = self.inner.borrow_mut();
        let Some(loaded) = inner.loaded.as_mut() else {
            return;
        };
        let (file, problems) = match failure {
            Failure::Invalid(problems) => (FileStatus::Invalid, problems.clone()),
            Failure::Unreadable(text) => (FileStatus::Unreadable, vec![text.clone()]),
        };
        loaded.file = file;
        loaded.problems = problems;
        if let Some(in_use) = loaded.in_use.as_mut()
            && in_use.origin == Origin::File
        {
            in_use.origin = Origin::LastValid;
        }
        drop(inner);
        self.publish();
    }

    /// hands the running core the change, unless it changes no pool.
    fn start_apply(&self, prepared: Prepared, previous: Option<Plan>) {
        let at = millis(self.clock.now());
        let mut inner = self.inner.borrow_mut();
        let mut info = ApplyInfo {
            revision: prepared.revision,
            at,
            outcome: "applied".to_string(),
            reason: None,
        };
        let changes = prepared
            .effects
            .iter()
            .any(|effect| effect.change != Change::Unchanged);
        let order = self.orders.borrow().clone();
        if let (true, Some(orders)) = (changes, order) {
            let removed = previous
                .map(|plan| plan.entries)
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| {
                    prepared
                        .effects
                        .iter()
                        .any(|effect| effect.pool == entry.name && effect.change == Change::Removed)
                })
                .collect();
            inner.applying = Some(Applying {
                removed,
                effects: prepared.effects.clone(),
            });
            "applying".clone_into(&mut info.outcome);
            let _ = orders.send(ApplyOrder {
                plan: prepared.plan,
                effects: prepared.effects,
            });
        }
        inner.last_apply = Some(info);
    }

    /// records how the running apply ended: `None` when it applied, or why it failed.
    pub(super) fn apply_ended(&self, failure: Option<String>) {
        self.end_apply(failure.map_or(("applied", None), |why| ("failed", Some(why))));
    }

    /// records that a stop, a quit or a failure of the core ended the running apply.
    pub(super) fn apply_cut_short(&self, outcome: &'static str, reason: Option<String>) {
        self.end_apply((outcome, reason));
    }

    fn end_apply(&self, (outcome, reason): (&'static str, Option<String>)) {
        {
            let mut inner = self.inner.borrow_mut();
            if inner.applying.take().is_none() {
                return;
            }
            if let Some(info) = inner.last_apply.as_mut() {
                outcome.clone_into(&mut info.outcome);
                info.reason = reason;
            }
        }
        self.publish();
    }

    /// the answer to a check or a change: the revision the file has after it, each
    /// pool's effect and the repositories no pool names any more.
    fn change_result(&self, prepared: &Prepared, mode: Mode) -> Json {
        // a pool skipped for want of a token, or that could not be prepared, has no
        // runner to stop
        let with_runners: BTreeSet<String> = self
            .board
            .slot_rows()
            .iter()
            .filter_map(|row| {
                row.name
                    .rsplit_once(" slot ")
                    .map(|(pool, _)| pool.to_string())
            })
            .collect();
        let effects: Vec<Json> = prepared
            .effects
            .iter()
            .map(|effect| {
                json!({
                    "pool": effect.pool,
                    "change": effect.change,
                    "runners_before": effect.runners_before,
                    "runners_after": effect.runners_after,
                    "stops_runners": mode == Mode::Apply
                        && effect.stops_runners()
                        && with_runners.contains(&effect.pool),
                })
            })
            .collect();
        json!({
            "revision": prepared.revision,
            "effects": effects,
            "repositories_released": prepared.released,
            "applies": mode == Mode::Apply,
        })
    }

    /// the checkout above the configuration file, which a diagnostic directory must
    /// lie outside.
    fn above(&self) -> Option<PathBuf> {
        checkout::find(&self.config_path())
    }

    /// the checks the agent loads with.
    fn checks<'p>(&self, above: Option<&'p std::path::Path>) -> StandardChecks<'p> {
        StandardChecks {
            rule: CheckoutRule::AboveAndAtSink(above),
        }
    }
}

/// a `config_read` result, without its values when they would not fit one message.
fn read_result(
    revision: &str,
    status: &str,
    problems: &[String],
    values: Option<(serde_json::Map<String, Json>, Vec<Json>)>,
) -> Json {
    let problems: Vec<Problem> = problems
        .iter()
        .take(MAX_PROBLEMS)
        .map(|text| problem(text))
        .collect();
    let result = |(settings, pools): (serde_json::Map<String, Json>, Vec<Json>), too_large| {
        json!({
            "revision": revision,
            "status": status,
            "settings": settings,
            "pools": pools,
            "problems": problems,
            "too_large": too_large,
        })
    };
    let full = result(values.unwrap_or_default(), false);
    let size = serde_json::to_vec(&full).map_or(usize::MAX, |bytes| bytes.len());
    if size > MAX_AGENT_MESSAGE_BYTES - RESPONSE_ROOM {
        result(Default::default(), true)
    } else {
        full
    }
}
