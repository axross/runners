//! the notification-area icon, apart from the web view: its provisional images, when its
//! image is replaced, and what the platform says of the taskbar and the clock.

mod platform;
mod provisional;
mod watch;

pub use platform::{taskbar_reading, utc_offset_minutes};
pub use provisional::{Bitmap, SIZES, bitmap};
pub use watch::{Look, Polled, TaskbarReading, Watch, size_for_scale};
