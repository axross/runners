//! `stop`, `restart` and `status`: the clients that talk to this user's instance
//! over the control channel.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::instance::channel::{ClientError, exchange};
use crate::instance::protocol::{Outcome, Request, Response};
use crate::instance::roots::Roots;

/// How long `status` waits for its answer.
const STATUS_ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// How long `restart` waits for the instance to stop its containers, read its
/// configuration again and answer.
const RESTART_ANSWER_WITHIN: Duration = Duration::from_secs(35);

/// How long `stop` waits for the instance to end.
const STOP_WAIT: Duration = Duration::from_secs(35);

/// How often `stop` asks whether the instance is still there.
const STOP_POLL: Duration = Duration::from_millis(200);

const EXIT_OK: u8 = 0;
const EXIT_FAILED: u8 = 1;

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
        Err(error) => return failed(err, &error),
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

fn failed(err: &mut dyn Write, error: &dyn std::fmt::Display) -> u8 {
    let _ = writeln!(err, "error: {error}");
    EXIT_FAILED
}

fn no_answer(err: &mut dyn Write, error: &ClientError) -> u8 {
    match error {
        ClientError::NoInstance => {
            let _ = writeln!(err, "No instance is running.");
            EXIT_FAILED
        }
        _ => failed(err, error),
    }
}

fn refused(err: &mut dyn Write, response: &Response) -> u8 {
    failed(err, &format!("the instance refused: {}", response.message))
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

/// Asks the instance to stop, then waits up to [`STOP_WAIT`] for its channel to
/// close, which it does as it ends.
fn stop(roots: &Roots, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let started = Instant::now();
    match exchange(roots, &Request::Stop, STATUS_ANSWER_WITHIN) {
        Ok(response) if response.outcome == Outcome::Ok => {}
        Ok(response) => return refused(err, &response),
        Err(error) => return no_answer(err, &error),
    }
    let _ = writeln!(out, "Stopping the instance.");
    while started.elapsed() < STOP_WAIT {
        let left = STOP_WAIT.saturating_sub(started.elapsed());
        if let Err(ClientError::NoInstance) =
            exchange(roots, &Request::Status, left.min(STATUS_ANSWER_WITHIN))
        {
            let _ = writeln!(out, "The instance has stopped.");
            return EXIT_OK;
        }
        std::thread::sleep(STOP_POLL.min(STOP_WAIT.saturating_sub(started.elapsed())));
    }
    failed(
        err,
        &format!(
            "the instance has not ended within {} s",
            STOP_WAIT.as_secs()
        ),
    )
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
