//! the client side of the channel: connect, send one request, read one answer.

use std::fmt;
use std::io::{BufReader, Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::super::protocol::{Request, Response};
use super::super::roots::Roots;

/// a client gives up connecting after this long and treats it as no instance.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// why a client got no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// nothing answers at the channel: it does not exist, refuses, or did not accept
    /// within [`CONNECT_TIMEOUT`].
    NoInstance,
    /// the instance accepted the request and did not answer in time.
    NoAnswer,
    /// the exchange failed for another reason.
    Failed(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInstance => f.write_str("no instance is running"),
            Self::NoAnswer => f.write_str("the instance did not answer in time"),
            Self::Failed(detail) => write!(f, "the instance could not be reached: {detail}"),
        }
    }
}

impl std::error::Error for ClientError {}

enum Event {
    Connected,
    Answered(Result<Response, ClientError>),
}

/// sends `request` to this user's instance and waits up to `answer_within` for its
/// answer, after connecting for up to [`CONNECT_TIMEOUT`], on a thread of its own so
/// a connection that never completes ends the call at the limit rather than when
/// the operating system gives up.
pub fn exchange(
    roots: &Roots,
    request: &Request,
    answer_within: Duration,
) -> Result<Response, ClientError> {
    let roots = roots.clone();
    exchange_over(
        move || super::platform_connect(&roots),
        &request.encode(),
        CONNECT_TIMEOUT,
        answer_within,
    )
}

fn exchange_over<C, F>(
    connect: F,
    line: &str,
    connect_within: Duration,
    answer_within: Duration,
) -> Result<Response, ClientError>
where
    C: Read + Write,
    F: FnOnce() -> Result<C, ClientError> + Send + 'static,
{
    let line = line.to_string();
    let (events, inbox) = mpsc::channel();
    thread::spawn(move || {
        let outcome = (|| {
            let mut connection = connect()?;
            let _ = events.send(Event::Connected);
            connection
                .write_all(line.as_bytes())
                .and_then(|()| connection.flush())
                .map_err(|error| ClientError::Failed(error.to_string()))?;
            Response::read(&mut BufReader::new(connection))
                .map_err(|error| ClientError::Failed(error.to_string()))
        })();
        let _ = events.send(Event::Answered(outcome));
    });
    match inbox.recv_timeout(connect_within) {
        Ok(Event::Connected) => {}
        Ok(Event::Answered(outcome)) => return outcome,
        Err(_) => return Err(ClientError::NoInstance),
    }
    match inbox.recv_timeout(answer_within) {
        Ok(Event::Answered(outcome)) => outcome,
        Ok(Event::Connected) | Err(_) => Err(ClientError::NoAnswer),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor};
    use std::time::Instant;

    use super::*;

    /// a connection that reads a fixed answer and swallows what is written.
    struct Scripted(Cursor<Vec<u8>>);

    impl Read for Scripted {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.0.read(buffer)
        }
    }

    impl Write for Scripted {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_connection_that_never_completes_gives_up_at_the_connect_limit() {
        let started = Instant::now();
        let outcome = exchange_over(
            || -> Result<Scripted, ClientError> {
                thread::sleep(Duration::from_secs(30));
                Err(ClientError::NoInstance)
            },
            "status\n",
            CONNECT_TIMEOUT,
            Duration::from_secs(30),
        );
        let waited = started.elapsed();
        assert_eq!(outcome, Err(ClientError::NoInstance));
        assert!(waited >= CONNECT_TIMEOUT, "{waited:?}");
        assert!(
            waited < CONNECT_TIMEOUT + Duration::from_secs(2),
            "{waited:?}"
        );
    }

    #[test]
    fn the_connect_limit_is_two_seconds() {
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(2));
    }

    #[test]
    fn a_connection_that_is_accepted_and_never_answered_is_no_answer() {
        struct Silent;
        impl Read for Silent {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                thread::sleep(Duration::from_secs(5));
                Ok(0)
            }
        }
        impl Write for Silent {
            fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
                Ok(buffer.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let outcome = exchange_over(
            || Ok(Silent),
            "status\n",
            CONNECT_TIMEOUT,
            Duration::from_millis(100),
        );
        assert_eq!(outcome, Err(ClientError::NoAnswer));
    }

    #[test]
    fn an_answer_is_returned_whole() {
        let answer = Response::ok("running").with("pools", "2").encode();
        let outcome = exchange_over(
            move || Ok(Scripted(Cursor::new(answer.into_bytes()))),
            "status\n",
            CONNECT_TIMEOUT,
            Duration::from_secs(5),
        );
        assert_eq!(outcome, Ok(Response::ok("running").with("pools", "2")));
    }
}
