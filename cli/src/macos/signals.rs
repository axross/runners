//! the signal-source seam the instance polls: whether the Mac slept, the current
//! primary network, and the keep-awake hold. The real macOS source is
//! [`MacSignals`]; the tests use [`FakeSignals`](super::fake::FakeSignals), and a
//! debug build can be driven from outside through `MARSHAL_TEST_SIGNALS`.

use std::ffi::OsStr;
use std::future::Future;
use std::rc::Rc;

use super::keep_awake::HoldEvent;
use super::mac::MacSignals;
use super::network::NetworkReading;
#[cfg(debug_assertions)]
use super::scripted::ScriptedSignals;

/// the variable that names the directory a debug build reads its signals from; a
/// release build ignores it.
pub const TEST_SIGNALS_VARIABLE: &str = "MARSHAL_TEST_SIGNALS";

pub trait SignalSource {
    /// samples the clocks: true when the Mac slept, or its wall clock was stepped,
    /// since the sample before. The instance calls it every
    /// [`SAMPLE_INTERVAL`](super::sleep::SAMPLE_INTERVAL).
    fn sleep_detected(&self) -> bool;

    /// reads the primary network, as the instance does every
    /// [`CHECK_INTERVAL`](super::watch::CHECK_INTERVAL). A table that cannot be read is
    /// `NoDefaultRoute`.
    fn read_network(&self) -> impl Future<Output = NetworkReading>;

    /// takes the keep-awake hold, on entering running.
    fn hold_keep_awake(&self) -> HoldEvent;

    /// checks the hold, and retries a lost one.
    fn poll_keep_awake(&self) -> HoldEvent;

    /// ends the hold, on entering stopped.
    fn release_keep_awake(&self);

    /// the warning while the hold is wanted and not held.
    fn keep_awake_warning(&self) -> Option<String>;
}

impl<S: SignalSource + ?Sized> SignalSource for Rc<S> {
    fn sleep_detected(&self) -> bool {
        (**self).sleep_detected()
    }

    fn read_network(&self) -> impl Future<Output = NetworkReading> {
        (**self).read_network()
    }

    fn hold_keep_awake(&self) -> HoldEvent {
        (**self).hold_keep_awake()
    }

    fn poll_keep_awake(&self) -> HoldEvent {
        (**self).poll_keep_awake()
    }

    fn release_keep_awake(&self) {
        (**self).release_keep_awake();
    }

    fn keep_awake_warning(&self) -> Option<String> {
        (**self).keep_awake_warning()
    }
}

/// the directory a debug build reads its signals from: `requested` in a debug build,
/// and never in a release build.
#[must_use]
pub fn resolve_scripted_directory(requested: Option<&OsStr>) -> Option<std::path::PathBuf> {
    if cfg!(debug_assertions) {
        requested.map(std::path::PathBuf::from)
    } else {
        None
    }
}

/// the source this process polls: the real macOS one, or in a debug build the one
/// scripted through a directory.
pub enum Signals {
    Mac(Box<MacSignals>),
    #[cfg(debug_assertions)]
    Scripted(ScriptedSignals),
}

impl Signals {
    /// the source for `MARSHAL_TEST_SIGNALS`, else the real one with its
    /// `MARSHAL_TEST_*` program overrides.
    #[must_use]
    pub fn from_environment() -> Self {
        Self::from_scripted_directory(std::env::var_os(TEST_SIGNALS_VARIABLE).as_deref())
    }

    /// the source for a requested directory, which a release build ignores.
    #[must_use]
    pub fn from_scripted_directory(requested: Option<&OsStr>) -> Self {
        #[cfg(debug_assertions)]
        if let Some(directory) = resolve_scripted_directory(requested) {
            return Self::Scripted(ScriptedSignals::new(directory));
        }
        #[cfg(not(debug_assertions))]
        let _ = requested;
        Self::Mac(Box::new(MacSignals::from_environment()))
    }

    /// whether this source is scripted rather than the real one.
    #[must_use]
    pub fn is_scripted(&self) -> bool {
        match self {
            Self::Mac(_) => false,
            #[cfg(debug_assertions)]
            Self::Scripted(_) => true,
        }
    }
}

impl SignalSource for Signals {
    fn sleep_detected(&self) -> bool {
        match self {
            Self::Mac(source) => source.sleep_detected(),
            #[cfg(debug_assertions)]
            Self::Scripted(source) => source.sleep_detected(),
        }
    }

    async fn read_network(&self) -> NetworkReading {
        match self {
            Self::Mac(source) => source.read_network().await,
            #[cfg(debug_assertions)]
            Self::Scripted(source) => source.read_network().await,
        }
    }

    fn hold_keep_awake(&self) -> HoldEvent {
        match self {
            Self::Mac(source) => source.hold_keep_awake(),
            #[cfg(debug_assertions)]
            Self::Scripted(source) => source.hold_keep_awake(),
        }
    }

    fn poll_keep_awake(&self) -> HoldEvent {
        match self {
            Self::Mac(source) => source.poll_keep_awake(),
            #[cfg(debug_assertions)]
            Self::Scripted(source) => source.poll_keep_awake(),
        }
    }

    fn release_keep_awake(&self) {
        match self {
            Self::Mac(source) => source.release_keep_awake(),
            #[cfg(debug_assertions)]
            Self::Scripted(source) => source.release_keep_awake(),
        }
    }

    fn keep_awake_warning(&self) -> Option<String> {
        match self {
            Self::Mac(source) => source.keep_awake_warning(),
            #[cfg(debug_assertions)]
            Self::Scripted(source) => source.keep_awake_warning(),
        }
    }
}
