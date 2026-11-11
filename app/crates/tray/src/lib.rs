//! the notification-area icon, apart from the web view: its images, when its
//! image is replaced, and what the platform says of the taskbar and the clock.

mod glyphs;
mod platform;
mod surface;
mod watch;

pub use glyphs::{Bitmap, SIZES, bitmap};
pub use platform::{taskbar_reading, utc_offset_minutes};
pub use surface::{Changes, Surface, menu_id};
pub use watch::{Look, Polled, TaskbarReading, Watch, size_for_scale};
