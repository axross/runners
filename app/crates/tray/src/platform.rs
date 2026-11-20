//! what the operating system says of the taskbar and the clock, read without unsafe
//! code. Windows reads both from the registry; the Mac has no taskbar mode and reads
//! its offset from Core Foundation; elsewhere the mode is absent and local time is
//! taken as UTC.

use crate::watch::TaskbarReading;

#[cfg(windows)]
mod windows {
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE};

    use crate::watch::TaskbarReading;

    const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    const TIME_ZONE: &str = r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation";

    /// `ERROR_FILE_NOT_FOUND` as an `HRESULT`, which is what a missing key or value is.
    const NOT_FOUND: i32 = 0x8007_0002_u32.cast_signed();

    pub fn taskbar_reading() -> TaskbarReading {
        let read = CURRENT_USER
            .open(PERSONALIZE)
            .and_then(|key| key.get_u32("SystemUsesLightTheme"));
        match read {
            Ok(value) => TaskbarReading::Value(value),
            Err(error) if error.code().0 == NOT_FOUND => TaskbarReading::Absent,
            Err(error) => TaskbarReading::Failed(error.to_string()),
        }
    }

    pub fn utc_offset_minutes() -> Option<i32> {
        let bias = LOCAL_MACHINE
            .open(TIME_ZONE)
            .and_then(|key| key.get_u32("ActiveTimeBias"))
            .ok()?;
        // Windows stores the bias, minutes to add to local time to get UTC, as a signed
        // value in a DWORD
        bias.cast_signed().checked_neg()
    }
}

/// the taskbar's mode as the registry holds it.
#[must_use]
pub fn taskbar_reading() -> TaskbarReading {
    #[cfg(windows)]
    {
        windows::taskbar_reading()
    }
    #[cfg(target_os = "macos")]
    {
        TaskbarReading::NoMode
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        TaskbarReading::Absent
    }
}

/// how many minutes local time is ahead of UTC now, daylight saving included; 0 where it
/// cannot be read.
#[must_use]
pub fn utc_offset_minutes() -> i32 {
    #[cfg(windows)]
    {
        windows::utc_offset_minutes().unwrap_or(0)
    }
    #[cfg(target_os = "macos")]
    {
        macos::utc_offset_minutes()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        0
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::date::CFDate;
    use core_foundation::timezone::CFTimeZone;

    /// the system time zone's offset from UTC now, which Core Foundation gives in whole
    /// seconds; it keeps the zone the app started in until the app restarts.
    pub fn utc_offset_minutes() -> i32 {
        let seconds = CFTimeZone::system().seconds_from_gmt(CFDate::now());
        (seconds / 60.0).round() as i32
    }
}
