//! the image check as the runtime runs it: through the backend, onto the board.

use crate::image::check::ImageCheck;

use super::backend::Backend;
use super::board::Board;

/// checks the configured image and records what the check found for `status`.
pub async fn check_image<B: Backend>(backend: &B, board: &Board, image: &str) -> ImageCheck {
    let check = backend.check_image(image).await;
    board.record_image(image, &check);
    check
}
