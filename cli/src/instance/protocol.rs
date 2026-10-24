//! The text protocol of the control channel.
//!
//! A request is one UTF-8 line: `status`, `stop`, `restart`, or `resume <profile>
//! <path>`, where the path is the rest of the line. A response is one or more lines
//! that end with a line `end`: the first is `ok` or `refused`, with an optional
//! message, and each further line is a field name, a space and its value. A
//! response carries states, reasons, and names, never a token or a JIT
//! configuration, and no request carries a path for the instance to read: a resume
//! names the path only so the instance can compare it with its own.

use std::fmt;
use std::io::{self, BufRead, Read};
use std::path::Path;

use tokio::sync::{mpsc, oneshot};

use crate::profile::Profile;

/// The line that ends a response.
pub const END: &str = "end";

/// The longest request line, including its line break. A path is at most a few
/// kilobytes, so this only stops a client from making the instance buffer without
/// limit.
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;

/// The longest response a client reads.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// What a client asks of the instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Status,
    Stop,
    Restart,
    Resume(ResumeRequest),
}

/// The profile and path `start` was given, for the instance to compare with its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeRequest {
    pub profile: Profile,
    pub path: String,
}

/// Why a request line cannot be sent or understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    /// The line is not one of the four requests.
    Unknown,
    /// The line is not UTF-8 text.
    NotText,
    /// The line is longer than [`MAX_REQUEST_BYTES`].
    TooLong,
    /// A path to resume with has a line break or is not UTF-8.
    UnsendablePath,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unknown => "the request is not status, stop, restart, or resume",
            Self::NotText => "the request is not UTF-8 text",
            Self::TooLong => "the request is too long",
            Self::UnsendablePath => {
                "the configuration path has a line break or is not UTF-8 text, so it cannot be sent"
            }
        })
    }
}

impl std::error::Error for RequestError {}

impl Request {
    /// A resume for `path`, which should already be canonical.
    ///
    /// # Errors
    ///
    /// A path that is not UTF-8, or holds a line break, cannot be a line of the
    /// protocol, so `start` refuses it before it connects.
    pub fn resume(profile: Profile, path: &Path) -> Result<Self, RequestError> {
        let text = path.to_str().ok_or(RequestError::UnsendablePath)?;
        if text.is_empty() || text.contains(['\n', '\r']) {
            return Err(RequestError::UnsendablePath);
        }
        Ok(Self::Resume(ResumeRequest {
            profile,
            path: text.to_string(),
        }))
    }

    /// The request as the line a client sends, with its line break.
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            Self::Status => "status\n".to_string(),
            Self::Stop => "stop\n".to_string(),
            Self::Restart => "restart\n".to_string(),
            Self::Resume(resume) => format!("resume {} {}\n", resume.profile.name(), resume.path),
        }
    }

    /// Reads one request line, with or without its line break.
    ///
    /// # Errors
    ///
    /// See [`RequestError`].
    pub fn parse(line: &[u8]) -> Result<Self, RequestError> {
        if line.len() > MAX_REQUEST_BYTES {
            return Err(RequestError::TooLong);
        }
        let text = std::str::from_utf8(line).map_err(|_| RequestError::NotText)?;
        let text = text.strip_suffix('\n').unwrap_or(text);
        match text {
            "status" => return Ok(Self::Status),
            "stop" => return Ok(Self::Stop),
            "restart" => return Ok(Self::Restart),
            _ => {}
        }
        let rest = text.strip_prefix("resume ").ok_or(RequestError::Unknown)?;
        let (profile, path) = rest.split_once(' ').ok_or(RequestError::Unknown)?;
        let profile = Profile::from_name(profile).ok_or(RequestError::Unknown)?;
        if path.is_empty() || path.contains(['\n', '\r']) {
            return Err(RequestError::Unknown);
        }
        Ok(Self::Resume(ResumeRequest {
            profile,
            path: path.to_string(),
        }))
    }
}

/// Reads one line of at most `limit` bytes, keeping its line break. Returns `None`
/// at the end of the stream before any byte.
///
/// # Errors
///
/// A line that does not end within `limit` bytes is [`io::ErrorKind::InvalidData`].
pub fn read_line_limited<R: BufRead>(reader: &mut R, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let read = reader
        .by_ref()
        .take(limit as u64)
        .read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line_is_cut(&line, limit) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the line is too long",
        ));
    }
    Ok(Some(line))
}

/// Whether `line`, read with a limit of `limit` bytes, stopped at the limit before
/// its line break, so the line is longer than the limit allows.
#[must_use]
pub fn line_is_cut(line: &[u8], limit: usize) -> bool {
    line.last() != Some(&b'\n') && line.len() >= limit
}

/// Whether the instance did what was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Refused,
}

/// What the instance answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub outcome: Outcome,
    pub message: String,
    fields: Vec<(String, String)>,
}

/// Why a response cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseError {
    /// The response ended before its `end` line.
    Truncated,
    /// The first line is neither `ok` nor `refused`.
    Malformed,
    /// A line is not UTF-8 text.
    NotText,
}

impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Truncated => "the instance's answer ended early",
            Self::Malformed => "the instance's answer is not understood",
            Self::NotText => "the instance's answer is not UTF-8 text",
        })
    }
}

impl std::error::Error for ResponseError {}

/// A line break would end the line early, so it becomes a space.
fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

impl Response {
    #[must_use]
    pub fn ok(message: &str) -> Self {
        Self {
            outcome: Outcome::Ok,
            message: one_line(message),
            fields: Vec::new(),
        }
    }

    #[must_use]
    pub fn refused(message: &str) -> Self {
        Self {
            outcome: Outcome::Refused,
            message: one_line(message),
            fields: Vec::new(),
        }
    }

    /// The answer to a request the instance ended before it could take or answer.
    #[must_use]
    pub fn ending() -> Self {
        Self::refused("the instance is ending")
    }

    /// Adds a field. `name` is a fixed word without a space.
    #[must_use]
    pub fn with(mut self, name: &str, value: &str) -> Self {
        self.fields.push((name.to_string(), one_line(value)));
        self
    }

    /// The first field called `name`.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    #[must_use]
    pub fn fields(&self) -> &[(String, String)] {
        &self.fields
    }

    /// The response as the text the instance sends, ending with the `end` line.
    #[must_use]
    pub fn encode(&self) -> String {
        let word = match self.outcome {
            Outcome::Ok => "ok",
            Outcome::Refused => "refused",
        };
        let mut text = if self.message.is_empty() {
            format!("{word}\n")
        } else {
            format!("{word} {}\n", self.message)
        };
        for (name, value) in &self.fields {
            text.push_str(&format!("{name} {value}\n"));
        }
        text.push_str(END);
        text.push('\n');
        text
    }

    /// Reads a response up to its `end` line.
    ///
    /// # Errors
    ///
    /// See [`ResponseError`]; an I/O error reads as a truncated answer.
    pub fn read<R: BufRead>(reader: &mut R) -> Result<Self, ResponseError> {
        let mut consumed = 0usize;
        let mut next = |reader: &mut R| -> Result<String, ResponseError> {
            let limit = MAX_RESPONSE_BYTES.saturating_sub(consumed);
            let line = read_line_limited(reader, limit)
                .map_err(|_| ResponseError::Truncated)?
                .ok_or(ResponseError::Truncated)?;
            consumed += line.len();
            let text = String::from_utf8(line).map_err(|_| ResponseError::NotText)?;
            Ok(text.strip_suffix('\n').unwrap_or(&text).to_string())
        };
        let first = next(reader)?;
        let (word, message) = first.split_once(' ').unwrap_or((first.as_str(), ""));
        let outcome = match word {
            "ok" => Outcome::Ok,
            "refused" => Outcome::Refused,
            _ => return Err(ResponseError::Malformed),
        };
        let mut response = Self {
            outcome,
            message: message.to_string(),
            fields: Vec::new(),
        };
        loop {
            let line = next(reader)?;
            if line == END {
                return Ok(response);
            }
            let (name, value) = line.split_once(' ').unwrap_or((line.as_str(), ""));
            response.fields.push((name.to_string(), value.to_string()));
        }
    }
}

/// A request that reached the instance, and where its answer goes.
#[derive(Debug)]
pub struct Incoming {
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}

/// Hands the request on `line` to the instance and returns the receiver its answer
/// arrives on. Each transport waits on that receiver in its own way.
///
/// # Errors
///
/// The answer to send at once: the refusal of a line that is not a request, or
/// [`Response::ending`] when the instance takes no more requests.
pub fn submit(
    line: &[u8],
    requests: &mpsc::UnboundedSender<Incoming>,
) -> Result<oneshot::Receiver<Response>, Response> {
    let request = Request::parse(line).map_err(|error| Response::refused(&error.to_string()))?;
    let (reply, answer) = oneshot::channel();
    requests
        .send(Incoming { request, reply })
        .map_err(|_| Response::ending())?;
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_request_round_trips_through_its_line() {
        let resume =
            Request::resume(Profile::Axpc, Path::new("/etc/runners/host config.json")).unwrap();
        for request in [Request::Status, Request::Stop, Request::Restart, resume] {
            assert_eq!(Request::parse(request.encode().as_bytes()), Ok(request));
        }
    }

    #[test]
    fn a_resume_path_is_the_rest_of_the_line_and_may_hold_spaces() {
        assert_eq!(
            Request::parse(b"resume axmc /a b  c\n"),
            Ok(Request::Resume(ResumeRequest {
                profile: Profile::Axmc,
                path: "/a b  c".to_string()
            }))
        );
    }

    #[test]
    fn a_path_with_a_line_break_or_bad_text_is_refused_before_it_is_sent() {
        for path in ["/a\nb", "/a\rb", ""] {
            assert_eq!(
                Request::resume(Profile::Axpc, Path::new(path)),
                Err(RequestError::UnsendablePath),
                "{path:?}"
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let path = Path::new(std::ffi::OsStr::from_bytes(b"/a\xFFb"));
            assert_eq!(
                Request::resume(Profile::Axpc, path),
                Err(RequestError::UnsendablePath)
            );
        }
    }

    #[test]
    fn anything_else_is_not_a_request() {
        for line in [
            &b""[..],
            b"STATUS\n",
            b"status now\n",
            b"stop \n",
            b"resume\n",
            b"resume axpc\n",
            b"resume axpc \n",
            b"resume other /path\n",
            b"resume axpc /a\rb\n",
        ] {
            assert_eq!(Request::parse(line), Err(RequestError::Unknown), "{line:?}");
        }
        assert_eq!(Request::parse(b"\xFF\n"), Err(RequestError::NotText));
        let long = vec![b'a'; MAX_REQUEST_BYTES + 1];
        assert_eq!(Request::parse(&long), Err(RequestError::TooLong));
    }

    #[test]
    fn submit_hands_a_request_on_and_answers_anything_else_at_once() {
        let (requests, mut instance) = mpsc::unbounded_channel();
        let answer = submit(b"status\n", &requests).unwrap();
        let incoming = instance.try_recv().unwrap();
        assert_eq!(incoming.request, Request::Status);
        incoming.reply.send(Response::ok("running")).unwrap();
        assert_eq!(answer.blocking_recv(), Ok(Response::ok("running")));

        let refusal = submit(b"STATUS\n", &requests).unwrap_err();
        assert_eq!(
            refusal,
            Response::refused(&RequestError::Unknown.to_string())
        );
        assert!(instance.try_recv().is_err());

        drop(instance);
        assert_eq!(
            submit(b"stop\n", &requests).unwrap_err(),
            Response::ending()
        );
    }

    #[test]
    fn a_response_ends_with_an_end_line_and_round_trips() {
        let response = Response::ok("running")
            .with("state", "running")
            .with("config", "/etc/a b.json");
        let text = response.encode();
        assert_eq!(
            text,
            "ok running\nstate running\nconfig /etc/a b.json\nend\n"
        );
        assert_eq!(Response::read(&mut text.as_bytes()), Ok(response));
        let refused = Response::refused("the instance is stopping");
        assert_eq!(
            Response::read(&mut refused.encode().as_bytes()),
            Ok(refused)
        );
        let bare = Response::ok("");
        assert_eq!(bare.encode(), "ok\nend\n");
    }

    #[test]
    fn a_line_break_in_a_value_cannot_end_the_response_early() {
        let response = Response::ok("x").with("reason", "a\nend\nb");
        let read = Response::read(&mut response.encode().as_bytes()).unwrap();
        assert_eq!(read.field("reason"), Some("a end b"));
    }

    #[test]
    fn a_truncated_or_malformed_response_is_an_error() {
        assert_eq!(
            Response::read(&mut &b"ok\nstate running\n"[..]),
            Err(ResponseError::Truncated)
        );
        assert_eq!(Response::read(&mut &b""[..]), Err(ResponseError::Truncated));
        assert_eq!(
            Response::read(&mut &b"maybe\nend\n"[..]),
            Err(ResponseError::Malformed)
        );
        assert_eq!(
            Response::read(&mut &b"ok\n\xFF\nend\n"[..]),
            Err(ResponseError::NotText)
        );
    }

    #[test]
    fn a_line_over_the_limit_is_an_error_and_one_at_the_limit_is_not() {
        let mut exact = vec![b'a'; 9];
        exact.push(b'\n');
        assert_eq!(
            read_line_limited(&mut &exact[..], 10).unwrap(),
            Some(exact.clone())
        );
        let long = [b'a'; 11];
        assert!(read_line_limited(&mut &long[..], 10).is_err());
        assert_eq!(read_line_limited(&mut &b""[..], 10).unwrap(), None);
    }

    /// A field line that, after `ok\n`, leaves `room` bytes of the response limit.
    fn field_leaving(room: usize) -> String {
        let value = "a".repeat(MAX_RESPONSE_BYTES - "ok\n".len() - "f \n".len() - room);
        format!("f {value}\n")
    }

    #[test]
    fn a_response_of_exactly_the_limit_is_read() {
        let text = format!("ok\n{}end\n", field_leaving("end\n".len()));
        assert_eq!(text.len(), MAX_RESPONSE_BYTES);
        let read = Response::read(&mut text.as_bytes()).unwrap();
        assert_eq!(read.fields().len(), 1);
    }

    #[test]
    fn once_the_limit_is_reached_not_even_an_empty_line_is_read() {
        let tail = format!("{}end\n", "\n".repeat(1000));
        let text = format!("ok\n{}{tail}", field_leaving(0));
        let mut rest = text.as_bytes();
        assert_eq!(Response::read(&mut rest), Err(ResponseError::Truncated));
        assert_eq!(rest.len(), tail.len());
    }
}
