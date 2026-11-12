//! the app's session with its agent, kept free of the web view so it builds and is
//! tested everywhere: the connection lifecycle, the view it keeps current, the token
//! path, the launcher that starts the agent from the app's own executable, and the
//! app's log.

mod choose;
mod connect;
mod launcher;
mod library;
mod log;
mod seam;
mod session;
mod wall;

pub use choose::{Chosen, Confirmable, Then, choose};
pub use connect::RETRY_EVERY;
pub use launcher::OwnExecutable;
pub use library::{Agent, Library, dial_error, failure};
pub use log::{AppLog, KEPT_FILES, MAX_FILE_BYTES};
pub use seam::{
    Connected, Dial, DialError, Failure, Launcher, Link, Log, Method, Present, Pushed, Shown,
    TimeSource,
};
pub use session::{Act, Ended, Forms, GITHUB_ASK_AGAIN, Handle, RENEW_EVERY, Seams, Session};
pub use wall::WallClock;
