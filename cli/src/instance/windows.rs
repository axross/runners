//! what the Windows pipes share: the current user's SID, and the security
//! descriptor that lets only that user use a pipe, built from its text form so no
//! `unsafe` is needed; neither crate here reads a live pipe's descriptor back, so CI
//! reads it from PowerShell.

use std::ffi::OsStr;
use std::io;

use interprocess::os::windows::ToWtf16;
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use winsafe::{self as w, co};

/// `ERROR_ACCESS_DENIED`, which creating a second first instance of a pipe gives.
const ERROR_ACCESS_DENIED: i32 = 5;

/// `ERROR_PIPE_BUSY`, which creating an instance beyond the limit gives.
const ERROR_PIPE_BUSY: i32 = 231;

fn failed(call: &str, error: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("{call} failed: {error}"))
}

/// the security identifier of the user this process runs as, such as `S-1-5-21-...`;
/// fails when the process token cannot be read.
pub fn current_user_sid() -> io::Result<String> {
    let token = w::HPROCESS::GetCurrentProcess()
        .OpenProcessToken(co::TOKEN::QUERY)
        .map_err(|error| failed("OpenProcessToken", error))?;
    let information = token
        .GetTokenInformation(co::TOKEN_INFORMATION_CLASS::User)
        .map_err(|error| failed("GetTokenInformation", error))?;
    let w::TokenInfo::User(user) = information else {
        return Err(io::Error::other("the process token has no user"));
    };
    let sid = user
        .User
        .Sid()
        .ok_or_else(|| io::Error::other("the process token's user has no SID"))?;
    // the SID's Display writes the error text when the conversion fails, so the
    // conversion is called directly
    w::ConvertSidToStringSid(sid).map_err(|error| failed("ConvertSidToStringSid", error))
}

/// the text form of the descriptor that grants all access to `sid` and to no one
/// else, and does not inherit.
#[must_use]
fn only_user_descriptor_text(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
}

/// builds the descriptor that lets only `sid` use a pipe; fails when the text is not
/// accepted as a descriptor.
pub fn only_user_descriptor(sid: &str) -> io::Result<SecurityDescriptor> {
    let text = only_user_descriptor_text(sid);
    let wide = OsStr::new(&text).to_wtf_16().map_err(io::Error::other)?;
    SecurityDescriptor::deserialize(&wide)
}

/// whether creating a pipe failed because its name is taken or all instances are in
/// use.
#[must_use]
pub fn name_is_taken(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_user_has_a_sid_and_the_descriptor_grants_it_alone() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        assert_eq!(
            only_user_descriptor_text(&sid),
            format!("D:P(A;;GA;;;{sid})")
        );
        only_user_descriptor(&sid).unwrap();
    }
}
