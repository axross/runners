//! `stop`, `restart` and `status`: the clients that talk to this user's instance
//! over the control channel.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::command::{EXIT_FAILED, EXIT_OK, print_error};
use crate::instance::channel::{ClientError, exchange};
use crate::instance::lock::{InstanceLock, LockError};
use crate::instance::protocol::{Outcome, Request, Response};
use crate::instance::roots::Roots;

/// How long `status` waits for its answer.
const STATUS_ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// How long `restart` waits for the instance to stop its containers, read its
/// configuration again and answer.
const RESTART_ANSWER_WITHIN: Duration = Duration::from_secs(35);

/// How long `stop` waits for the instance to end, counted from its request.
const STOP_WAIT: Duration = Duration::from_secs(35);

/// How often `stop` checks whether the instance's lock is free.
const STOP_POLL: Duration = Duration::from_millis(200);

/// A client command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    Stop,
    Restart,
    Status,
}

/// Runs the client and returns its exit code. `test_root` moves the channel's
/// roots in a debug build only.
pub fn run_client(
    client: Client,
    test_root: Option<&Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let roots = match Roots::resolve(test_root) {
        Ok(roots) => roots,
        Err(error) => return print_error(err, &error),
    };
    match client {
        Client::Status => match exchange(&roots, &Request::Status, STATUS_ANSWER_WITHIN) {
            Ok(response) => print_status(&response, out, err),
            Err(error) => no_answer(err, &error),
        },
        Client::Restart => match exchange(&roots, &Request::Restart, RESTART_ANSWER_WITHIN) {
            Ok(response) if response.outcome == Outcome::Ok => {
                let _ = writeln!(out, "The instance restarted and is running.");
                EXIT_OK
            }
            Ok(response) => refused(err, &response),
            Err(error) => no_answer(err, &error),
        },
        Client::Stop => stop(&roots, out, err),
    }
}

fn no_answer(err: &mut dyn Write, error: &ClientError) -> u8 {
    match error {
        ClientError::NoInstance => {
            let _ = writeln!(err, "No instance is running.");
            EXIT_FAILED
        }
        _ => print_error(err, error),
    }
}

fn refused(err: &mut dyn Write, response: &Response) -> u8 {
    print_error(err, &format!("the instance refused: {}", response.message))
}

/// Prints each field as `name: value`.
fn print_status(response: &Response, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    if response.outcome == Outcome::Refused {
        return refused(err, response);
    }
    for (name, value) in response.fields() {
        let _ = writeln!(out, "{name}: {value}");
    }
    EXIT_OK
}

/// Asks the instance to stop, then waits until [`STOP_WAIT`] after the request for
/// its lock to be free, which the operating system makes it as the process ends. It
/// tells by taking the lock and letting it go at once, so a `start` right after a
/// successful `stop` finds the lock free.
fn stop(roots: &Roots, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let started = Instant::now();
    match exchange(roots, &Request::Stop, STATUS_ANSWER_WITHIN) {
        Ok(response) if response.outcome == Outcome::Ok => {}
        Ok(response) => return refused(err, &response),
        Err(error) => return no_answer(err, &error),
    }
    let _ = writeln!(out, "Stopping the instance.");
    loop {
        match InstanceLock::acquire(roots) {
            Ok(_free) => {
                let _ = writeln!(out, "The instance has stopped.");
                return EXIT_OK;
            }
            Err(LockError::Held) => {}
            Err(error) => return print_error(err, &error),
        }
        let left = STOP_WAIT.saturating_sub(started.elapsed());
        if left.is_zero() {
            return print_error(
                err,
                &format!(
                    "the instance has not ended within {} s",
                    STOP_WAIT.as_secs()
                ),
            );
        }
        std::thread::sleep(STOP_POLL.min(left));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clients_wait_5_35_and_35_seconds() {
        assert_eq!(STATUS_ANSWER_WITHIN, Duration::from_secs(5));
        assert_eq!(RESTART_ANSWER_WITHIN, Duration::from_secs(35));
        assert_eq!(STOP_WAIT, Duration::from_secs(35));
    }
}
