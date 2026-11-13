// what macOS draws differently: with option B's unified title bar the window buttons sit
// in the page's top band, so on macOS that band is a drag region and the page marks the
// root for the stylesheet. Elsewhere the window has its own title bar and nothing changes.

/** whether the page runs in macOS's web view. */
export const ON_MAC = navigator.userAgent.includes("Macintosh");

/**
 * the value of `data-tauri-drag-region` for a band the window is dragged by: the whole
 * band on macOS, where its controls still take their own clicks, and none elsewhere.
 */
export const DRAG_REGION = ON_MAC ? "deep" : undefined;

if (ON_MAC) {
  document.documentElement.dataset.platform = "macos";
}
