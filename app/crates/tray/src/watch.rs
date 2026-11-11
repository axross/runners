//! when the notification-area icon's image is replaced: when the icon's state changes,
//! when the taskbar's mode changes, and when the primary display's scaling changes, and
//! at no other time. The thread that polls once a second reads the platform and hands
//! it to [`Watch`], which decides; it holds nothing of the platform itself.

use marshal_view::{GlyphColour, IconState, TaskbarMode};

use crate::glyphs::SIZES;

/// what reading the taskbar's mode gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskbarReading {
    /// the value of `SystemUsesLightTheme`.
    Value(u32),
    /// there is no such value.
    Absent,
    /// the registry could not be read, for this reason.
    Failed(String),
}

/// what the icon looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub state: IconState,
    pub colour: GlyphColour,
    /// the image's size in pixels.
    pub size: u32,
}

/// the size of the image for a display scaling: the nearest of [`SIZES`] to 16 pixels
/// times the scaling, and the larger when two are equally near.
#[must_use]
pub fn size_for_scale(scale: f64) -> u32 {
    let wanted = 16.0 * scale;
    SIZES
        .iter()
        .copied()
        .min_by(|a, b| {
            let near = |size: &u32| (f64::from(*size) - wanted).abs();
            near(a).total_cmp(&near(b)).then_with(|| b.cmp(a))
        })
        .unwrap_or(SIZES[0])
}

/// the result of one poll.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Polled {
    /// the look to put on the icon, when it differs from the one on it.
    pub replace: Option<Look>,
    /// a line for the log, said once: the mode cannot be read, so the taskbar counts as
    /// dark.
    pub warn: Option<String>,
}

#[derive(Debug)]
pub struct Watch {
    state: IconState,
    mode: Option<TaskbarMode>,
    scale: f64,
    warned: bool,
    applied: Option<Look>,
}

impl Default for Watch {
    fn default() -> Self {
        Self::new()
    }
}

impl Watch {
    /// an icon that is not in control, on a dark taskbar at 100 percent, with nothing on
    /// it yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: IconState::NotInControl,
            mode: None,
            scale: 1.0,
            warned: false,
            applied: None,
        }
    }

    /// the look the icon has now, or is about to be given: the newest one decided. Whatever
    /// puts a look on the icon later, from another thread, reads this when it runs, so a
    /// look decided earlier cannot replace a newer one.
    #[must_use]
    pub fn current(&self) -> Look {
        Look {
            state: self.state,
            colour: GlyphColour::for_mode(self.mode),
            size: size_for_scale(self.scale),
        }
    }

    fn look(&mut self) -> Option<Look> {
        let look = self.current();
        if self.applied == Some(look) {
            return None;
        }
        self.applied = Some(look);
        Some(look)
    }

    /// the icon's state changed; the look to put on it, if it differs.
    pub fn set_state(&mut self, state: IconState) -> Option<Look> {
        self.state = state;
        self.look()
    }

    /// reads the taskbar and the display. An absent or unreadable mode counts as dark,
    /// and is logged once; a display whose scaling cannot be read keeps the last.
    pub fn poll(&mut self, reading: TaskbarReading, scale: Option<f64>) -> Polled {
        let (mode, trouble) = match &reading {
            TaskbarReading::Value(value) => match TaskbarMode::from_registry(Some(*value)) {
                Some(mode) => (Some(mode), None),
                None => (None, Some(format!("SystemUsesLightTheme is {value}"))),
            },
            TaskbarReading::Absent => (None, Some("there is no SystemUsesLightTheme".to_string())),
            TaskbarReading::Failed(why) => (None, Some(why.clone())),
        };
        self.mode = mode;
        if let Some(scale) = scale.filter(|scale| scale.is_finite() && *scale > 0.0) {
            self.scale = scale;
        }
        let warn = trouble
            .filter(|_| !self.warned)
            .inspect(|_| self.warned = true)
            .map(|why| {
                format!("The taskbar's mode cannot be read ({why}), so the icon is drawn for a dark taskbar.")
            });
        Polled {
            replace: self.look(),
            warn,
        }
    }
}
