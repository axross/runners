//! the notification-area icon: which of five shapes it takes, and the colour of its
//! glyph for the taskbar it sits on.

use serde::Serialize;

use crate::state::{Context, Resident};

/// the five states the icon's shape tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IconState {
    AllUp,
    SomeNotUp,
    Stopped,
    StoppedByError,
    NotInControl,
}

/// the icon's state; building shows stopped, and a stop for a reason this reader does
/// not know is not taken for an error.
pub fn shape(context: &Context<'_>) -> IconState {
    match &context.resident {
        Resident::StartingMarshal
        | Resident::NoAgent
        | Resident::NotAnswering
        | Resident::LockHeld
        | Resident::OutsideApplications
        | Resident::LostContact { .. }
        | Resident::Unknown(_) => IconState::NotInControl,
        Resident::Running if context.restart_due.is_some() => IconState::SomeNotUp,
        Resident::Running if context.wanted > 0 && context.up_settled >= context.wanted => {
            IconState::AllUp
        }
        Resident::Running | Resident::Resuming | Resident::Stopping | Resident::Quitting => {
            IconState::SomeNotUp
        }
        Resident::Building { .. } => IconState::Stopped,
        Resident::Stopped(stop) if stop.is_error() => IconState::StoppedByError,
        Resident::Stopped(_) => IconState::Stopped,
    }
}

/// whether the taskbar is dark or light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarMode {
    Dark,
    Light,
}

impl TaskbarMode {
    /// the mode from Windows' `SystemUsesLightTheme` value, which the app reads from the
    /// registry: 1 is light and 0 is dark. An absent value, or one that is neither,
    /// cannot be read, so it is `None`.
    #[must_use]
    pub fn from_registry(value: Option<u32>) -> Option<Self> {
        match value {
            Some(0) => Some(Self::Dark),
            Some(1) => Some(Self::Light),
            _ => None,
        }
    }
}

/// a colour as its red, green and blue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// the glyph's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GlyphColour {
    White,
    NearBlack,
}

impl GlyphColour {
    pub const WHITE: Rgb = Rgb(0xff, 0xff, 0xff);
    pub const NEAR_BLACK: Rgb = Rgb(0x1f, 0x1f, 0x1f);

    #[must_use]
    pub fn rgb(self) -> Rgb {
        match self {
            Self::White => Self::WHITE,
            Self::NearBlack => Self::NEAR_BLACK,
        }
    }

    /// white on a dark taskbar, and when the mode cannot be read; near-black on a light
    /// one.
    #[must_use]
    pub fn for_mode(mode: Option<TaskbarMode>) -> Self {
        match mode {
            Some(TaskbarMode::Light) => Self::NearBlack,
            Some(TaskbarMode::Dark) | None => Self::White,
        }
    }
}
