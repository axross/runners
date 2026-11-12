//! the notification-area icon, apart from the web view: its images, when its
//! image is replaced, and what the platform says of the taskbar and the clock.

mod app_icon;
mod encode;
mod glyphs;
mod platform;
mod surface;
mod watch;

pub use app_icon::{APP_ICON_SIZES, app_icon};
pub use encode::{ico, png, png_rgba};
pub use glyphs::{Bitmap, SIZES, bitmap};
pub use platform::{taskbar_reading, utc_offset_minutes};
pub use surface::{Changes, Surface, menu_id};
pub use watch::{Look, Polled, TaskbarReading, Watch, size_for_scale};
