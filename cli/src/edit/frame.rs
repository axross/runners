//! what the parser does not keep of a file's bytes: its byte-order mark, its line
//! ending, and whether its last line ends. the parser reads LF and drops a mark, so a
//! file is split into a frame and LF text before an edit and joined again after it.

const BYTE_ORDER_MARK: &str = "\u{feff}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    Lf,
    CrLf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Frame {
    byte_order_mark: bool,
    ending: LineEnding,
    final_newline: bool,
}

impl Frame {
    /// the frame of `bytes` and their text with LF line endings and no mark, or `None`
    /// when the bytes are not UTF-8. a file that mixes line endings gets the more
    /// common one, LF when the two are equally common.
    pub(super) fn split(bytes: &[u8]) -> Option<(Self, String)> {
        let text = std::str::from_utf8(bytes).ok()?;
        let (byte_order_mark, text) = match text.strip_prefix(BYTE_ORDER_MARK) {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let crlf = text.matches("\r\n").count();
        let lf = text.matches('\n').count().saturating_sub(crlf);
        let frame = Self {
            byte_order_mark,
            ending: if crlf > lf {
                LineEnding::CrLf
            } else {
                LineEnding::Lf
            },
            final_newline: text.ends_with('\n'),
        };
        Some((frame, text.replace("\r\n", "\n")))
    }

    /// the bytes of `text`, written with this frame's line ending and mark. the parser
    /// ends the last line it prints, so a file whose last line was not ended loses
    /// that newline again.
    pub(super) fn join(&self, text: &str) -> Vec<u8> {
        let text = if self.final_newline {
            text
        } else {
            text.strip_suffix('\n').unwrap_or(text)
        };
        let mut out = String::with_capacity(text.len() + 8);
        if self.byte_order_mark {
            out.push_str(BYTE_ORDER_MARK);
        }
        match self.ending {
            LineEnding::Lf => out.push_str(text),
            LineEnding::CrLf => out.push_str(&text.replace('\n', "\r\n")),
        }
        out.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(bytes: &[u8]) -> Vec<u8> {
        let (frame, text) = Frame::split(bytes).unwrap();
        frame.join(&text)
    }

    #[test]
    fn a_file_is_joined_as_it_was_split() {
        for bytes in [
            &b"a = 1\nb = 2\n"[..],
            b"a = 1\r\nb = 2\r\n",
            b"\xef\xbb\xbfa = 1\r\nb = 2\r\n",
            b"\xef\xbb\xbfa = 1\nb = 2\n",
            b"a = 1\nb = 2",
            b"a = 1\r\nb = 2",
            b"",
        ] {
            assert_eq!(round_trip(bytes), bytes);
        }
    }

    #[test]
    fn the_text_has_lf_endings_and_no_mark() {
        let (_, text) = Frame::split(b"\xef\xbb\xbfa = 1\r\nb = 2\r\n").unwrap();
        assert_eq!(text, "a = 1\nb = 2\n");
    }

    #[test]
    fn a_file_with_mixed_endings_is_joined_with_the_more_common_one() {
        let (frame, text) = Frame::split(b"a = 1\r\nb = 2\r\nc = 3\nd = 4\r\n").unwrap();
        assert_eq!(frame.join(&text), b"a = 1\r\nb = 2\r\nc = 3\r\nd = 4\r\n");
        let (frame, text) = Frame::split(b"a = 1\nb = 2\nc = 3\r\nd = 4\n").unwrap();
        assert_eq!(frame.join(&text), b"a = 1\nb = 2\nc = 3\nd = 4\n");
    }

    #[test]
    fn equally_common_endings_give_lf() {
        let (frame, text) = Frame::split(b"a = 1\r\nb = 2\n").unwrap();
        assert_eq!(frame.join(&text), b"a = 1\nb = 2\n");
    }

    #[test]
    fn a_line_added_after_an_unended_last_line_stays_unended() {
        let (frame, _) = Frame::split(b"a = 1").unwrap();
        assert_eq!(frame.join("a = 1\nb = 2\n"), b"a = 1\nb = 2");
    }

    #[test]
    fn bytes_that_are_not_utf8_have_no_text() {
        assert!(Frame::split(b"a = \xff\n").is_none());
    }
}
