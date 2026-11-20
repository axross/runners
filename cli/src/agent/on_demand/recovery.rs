use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::SystemTime;

use crate::runtime::token::TokenSource;
use crate::scaleset::{Client, Connection, Error, ErrorKind, ScaleSet, Session};
use crate::validate::PoolTarget;

use super::listener::Failure;

pub(crate) type PendingSessionCell = Rc<RefCell<Option<PendingSession>>>;
pub(crate) type SessionRecovery = BTreeMap<String, PendingSessionCell>;

#[derive(Clone)]
pub(crate) struct PendingSession {
    connection: Rc<Connection>,
    scale_set: ScaleSet,
    session: Option<Rc<Session>>,
    owner: String,
}

impl PendingSession {
    pub(super) fn creating(connection: Rc<Connection>, scale_set: ScaleSet, owner: String) -> Self {
        Self {
            connection,
            scale_set,
            session: None,
            owner,
        }
    }

    pub(super) fn known(
        connection: Rc<Connection>,
        scale_set: ScaleSet,
        session: Rc<Session>,
        owner: String,
    ) -> Self {
        Self {
            connection,
            scale_set,
            session: Some(session),
            owner,
        }
    }

    /// an unknown POST can only be settled by absence or by an exclusive new session, never a guessed DELETE.
    pub(crate) async fn retry<T: TokenSource>(
        pending: &PendingSessionCell,
        client: &Client,
        tokens: &T,
        now: SystemTime,
    ) -> Result<(), Failure> {
        let Some(old) = pending.borrow().clone() else {
            return Ok(());
        };
        let pat = tokens
            .token(
                &PoolTarget::Repository {
                    owner: old.connection.owner().to_owned(),
                    repository: old.connection.repository().to_owned(),
                },
                None,
            )
            .map_err(Failure::Store)?;
        let connection = if old.connection.needs_renewal(&pat, now) {
            Rc::new(
                client
                    .connect(old.connection.owner(), old.connection.repository(), &pat)
                    .await?,
            )
        } else {
            Rc::clone(&old.connection)
        };
        if !connection.same_service(&old.connection) {
            return Err(Failure::Api(Error::new(ErrorKind::OwnershipMismatch)));
        }
        let session = if let Some(session) = old.session {
            session
        } else {
            let set = match client
                .get_scale_set(&connection, old.scale_set.id(), old.scale_set.name())
                .await
            {
                Ok(set) => set,
                Err(error) if error.kind == ErrorKind::NotFound => {
                    pending.borrow_mut().take();
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            };
            let session = Rc::new(client.create_session(&connection, &set, &old.owner).await?);
            *pending.borrow_mut() = Some(Self::known(
                Rc::clone(&connection),
                set,
                Rc::clone(&session),
                old.owner,
            ));
            session
        };
        client.close_session(&connection, &session).await?;
        pending.borrow_mut().take();
        Ok(())
    }
}
