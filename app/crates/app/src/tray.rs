use tauri::AppHandle;
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use crate::app::show_main_window;

const OPEN: &str = "open";
const QUIT: &str = "quit";

const GLYPH_SIZE: u32 = 16;

/// a white ring, drawn as the decoded pixels the notification area takes, so the app
/// links no image decoder.
fn glyph() -> Image<'static> {
    let centre = (GLYPH_SIZE as f32 - 1.0) / 2.0;
    let mut pixels = Vec::with_capacity((GLYPH_SIZE * GLYPH_SIZE * 4) as usize);
    for y in 0..GLYPH_SIZE {
        for x in 0..GLYPH_SIZE {
            let distance = ((x as f32 - centre).powi(2) + (y as f32 - centre).powi(2)).sqrt();
            let alpha = if (4.5..=7.0).contains(&distance) {
                255
            } else {
                0
            };
            pixels.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    Image::new_owned(pixels, GLYPH_SIZE, GLYPH_SIZE)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, OPEN, "Open Marshal", true, None::<&str>)?,
            &MenuItem::with_id(app, QUIT, "Quit Marshal", true, None::<&str>)?,
        ],
    )?;
    TrayIconBuilder::with_id("marshal")
        .icon(glyph())
        .tooltip(marshal_view::tooltip(None))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        OPEN => show_main_window(app),
        QUIT => app.exit(0),
        _ => {}
    }
}
