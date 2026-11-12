//! the keep-awake request on Windows, held by one thread that lives as long as the
//! agent: Windows ties `SetThreadExecutionState` to the thread that called it, so the
//! request is taken and ended on that thread only. The library's policy admits no crate
//! that makes the call, so the executable supplies it as a [`Request`].

use std::any::Any;
use std::cell::RefCell;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub use crate::macos::keep_awake::{HoldEvent, RETRY_INTERVAL};

/// takes the request on the calling thread and returns a guard that ends it when it is
/// dropped there; the text says why Windows refused.
pub type Request = Box<dyn Fn() -> Result<Box<dyn Any>, String> + Send>;

enum Command {
    Hold(mpsc::Sender<Result<(), String>>),
    Release(mpsc::Sender<()>),
}

/// the thread that holds the request.
pub struct Holder {
    commands: mpsc::Sender<Command>,
}

impl Holder {
    /// starts the thread; the request is not held until [`hold`](Self::hold).
    pub fn spawn(request: Request) -> std::io::Result<Self> {
        let (commands, received) = mpsc::channel();
        thread::Builder::new()
            .name("keep-awake".to_string())
            .spawn(move || serve(&request, &received))?;
        Ok(Self { commands })
    }

    /// takes the request, or keeps the one already held.
    pub fn hold(&self) -> Result<(), String> {
        let ended = || "the keep-awake thread has ended".to_string();
        let (reply, answer) = mpsc::channel();
        self.commands
            .send(Command::Hold(reply))
            .map_err(|_| ended())?;
        answer.recv().unwrap_or_else(|_| Err(ended()))
    }

    /// ends the request, if one is held, before returning.
    pub fn release(&self) {
        let (reply, answer) = mpsc::channel();
        if self.commands.send(Command::Release(reply)).is_ok() {
            let _ = answer.recv();
        }
    }
}

fn serve(request: &Request, received: &mpsc::Receiver<Command>) {
    let mut held: Option<Box<dyn Any>> = None;
    // dropping the holder ends the loop, and the guard is dropped on this thread
    while let Ok(command) = received.recv() {
        match command {
            Command::Hold(reply) => {
                let result = match &held {
                    Some(_) => Ok(()),
                    None => request().map(|guard| held = Some(guard)),
                };
                let _ = reply.send(result);
            }
            Command::Release(reply) => {
                held = None;
                let _ = reply.send(());
            }
        }
    }
}

enum State {
    Released,
    Held,
    Failed { warning: String, retry_at: Instant },
}

/// the hold the instance takes on entering running and ends on entering stopped. A
/// refused request warns once and is asked again every `retry_interval`.
pub struct KeepAwake {
    holder: Holder,
    retry_interval: Duration,
    state: RefCell<State>,
}

impl KeepAwake {
    #[must_use]
    pub fn new(holder: Holder, retry_interval: Duration) -> Self {
        Self {
            holder,
            retry_interval,
            state: RefCell::new(State::Released),
        }
    }

    fn failed(&self, cause: &str) -> State {
        State::Failed {
            warning: warning_text(cause),
            retry_at: Instant::now() + self.retry_interval,
        }
    }

    /// takes the request; it does nothing while the request is wanted already.
    pub fn hold(&self) -> HoldEvent {
        let mut state = self.state.borrow_mut();
        if !matches!(*state, State::Released) {
            return HoldEvent::Unchanged;
        }
        match self.holder.hold() {
            Ok(()) => {
                *state = State::Held;
                HoldEvent::Unchanged
            }
            Err(cause) => {
                *state = self.failed(&cause);
                HoldEvent::Warned(warning_text(&cause))
            }
        }
    }

    /// asks again for a refused request once its retry interval has passed.
    pub fn poll(&self) -> HoldEvent {
        let mut state = self.state.borrow_mut();
        let State::Failed { retry_at, .. } = &*state else {
            return HoldEvent::Unchanged;
        };
        if Instant::now() < *retry_at {
            return HoldEvent::Unchanged;
        }
        match self.holder.hold() {
            Ok(()) => {
                *state = State::Held;
                HoldEvent::Recovered
            }
            Err(cause) => {
                *state = self.failed(&cause);
                HoldEvent::Unchanged
            }
        }
    }

    /// ends the request and stops asking for it.
    pub fn release(&self) {
        *self.state.borrow_mut() = State::Released;
        self.holder.release();
    }

    /// the warning while the request is wanted and refused.
    #[must_use]
    pub fn warning(&self) -> Option<String> {
        match &*self.state.borrow() {
            State::Failed { warning, .. } => Some(warning.clone()),
            _ => None,
        }
    }
}

fn warning_text(cause: &str) -> String {
    format!("keep-awake is not held ({cause}), so Windows may sleep when idle; trying again")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::ThreadId;
    use std::time::Duration;

    use super::*;

    /// records on which thread each request was taken and ended.
    #[derive(Default)]
    struct Record {
        events: Mutex<Vec<(&'static str, ThreadId)>>,
    }

    impl Record {
        fn push(&self, kind: &'static str) {
            self.events
                .lock()
                .unwrap()
                .push((kind, thread::current().id()));
        }
    }

    struct Guard(Arc<Record>);

    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.push("ended");
        }
    }

    fn request(record: &Arc<Record>, refuse: bool) -> Request {
        let record = Arc::clone(record);
        Box::new(move || {
            if refuse {
                return Err("refused".to_string());
            }
            record.push("taken");
            Ok(Box::new(Guard(Arc::clone(&record))) as Box<dyn Any>)
        })
    }

    #[test]
    fn the_request_is_taken_and_ended_on_one_thread_that_is_not_the_callers() {
        let record = Arc::new(Record::default());
        let holder = Holder::spawn(request(&record, false)).unwrap();
        holder.hold().unwrap();
        holder.hold().unwrap();
        holder.release();
        holder.release();
        holder.hold().unwrap();
        drop(holder);
        // the thread drops the last guard after the holder is gone
        for _ in 0..400 {
            if record.events.lock().unwrap().len() == 4 {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let events = record.events.lock().unwrap();
        let kinds: Vec<&str> = events.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(kinds, ["taken", "ended", "taken", "ended"]);
        let first = events[0].1;
        assert!(events.iter().all(|(_, thread)| *thread == first));
        assert_ne!(first, thread::current().id());
    }

    #[test]
    fn a_refusal_is_reported_and_holds_nothing() {
        let record = Arc::new(Record::default());
        let holder = Holder::spawn(request(&record, true)).unwrap();
        assert_eq!(holder.hold(), Err("refused".to_string()));
        holder.release();
        assert!(record.events.lock().unwrap().is_empty());
    }

    /// a request Windows refuses the first `refusals` times.
    fn refused_at_first(refusals: u32) -> (Request, Arc<AtomicU32>) {
        let asked = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&asked);
        let request: Request = Box::new(move || {
            if counter.fetch_add(1, Ordering::SeqCst) < refusals {
                Err("refused".to_string())
            } else {
                Ok(Box::new(()) as Box<dyn Any>)
            }
        });
        (request, asked)
    }

    #[test]
    fn a_refused_hold_warns_once_and_recovers_after_the_retry_interval() {
        let (request, asked) = refused_at_first(2);
        let retry = Duration::from_millis(40);
        let keep_awake = KeepAwake::new(Holder::spawn(request).unwrap(), retry);
        assert!(matches!(keep_awake.hold(), HoldEvent::Warned(_)));
        assert!(keep_awake.warning().unwrap().contains("refused"));
        // a second hold and a poll before the interval ask Windows nothing
        assert_eq!(keep_awake.hold(), HoldEvent::Unchanged);
        assert_eq!(keep_awake.poll(), HoldEvent::Unchanged);
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        thread::sleep(retry * 2);
        // the retry is refused again without a second warning
        assert_eq!(keep_awake.poll(), HoldEvent::Unchanged);
        assert!(keep_awake.warning().is_some());
        thread::sleep(retry * 2);
        assert_eq!(keep_awake.poll(), HoldEvent::Recovered);
        assert_eq!(keep_awake.warning(), None);
        assert_eq!(keep_awake.poll(), HoldEvent::Unchanged);
        assert_eq!(asked.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_release_stops_the_retries() {
        let (request, asked) = refused_at_first(1);
        let keep_awake = KeepAwake::new(Holder::spawn(request).unwrap(), Duration::ZERO);
        assert!(matches!(keep_awake.hold(), HoldEvent::Warned(_)));
        keep_awake.release();
        assert_eq!(keep_awake.warning(), None);
        assert_eq!(keep_awake.poll(), HoldEvent::Unchanged);
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }
}
