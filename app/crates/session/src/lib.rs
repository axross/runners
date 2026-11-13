//! the app's session with its agent, kept free of the web view so it builds and is
//! tested everywhere: the connection lifecycle, the view it keeps current, the token
//! path, the launcher that starts the agent from the app's own executable, the app's
//! log, and its part in an update: the feed check, the drain, and the record kept across
//! an update.

mod choose;
mod connect;
mod launcher;
mod library;
mod location;
mod log;
mod record;
mod seam;
mod session;
mod wall;

pub use choose::{Chosen, Confirmable, Then, choose};
pub use connect::RETRY_EVERY;
pub use launcher::OwnExecutable;
pub use library::{Agent, Library, dial_error, failure};
pub use location::{ANY_LOCATION_VARIABLE, in_applications, may_control_agent};
pub use log::{AppLog, KEPT_FILES, MAX_FILE_BYTES};
pub use record::{RECORD_APPLIES_FOR, RECORD_FILE, RecordFile};
pub use seam::{
    AgentInfo, Connected, Dial, DialError, Failure, Later, Launcher, Link, Log, Method, NoUpdates,
    Present, Process, Processes, Pushed, Record, Shown, TimeSource, Updater, Verified,
};
pub use session::{
    Act, CHECK_EVERY, EXIT_WITHIN, Ended, Forms, GITHUB_ASK_AGAIN, Handle, RENEW_EVERY, Seams,
    Session,
};
pub use wall::WallClock;
