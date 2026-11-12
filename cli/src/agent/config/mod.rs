//! what the configuration methods need besides the agent's state: the problems as
//! fields, the editable values of the file, the edits a client asks for, and the
//! effect of a new configuration on the pools of the one in use.

mod edits;
mod effects;
mod problem;
mod view;

pub use edits::{EditsError, read_edits};
pub use effects::{Change, Effect, effects};
pub use problem::{Problem, problem};
pub use view::editable;
