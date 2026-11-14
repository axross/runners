// marks the root on macOS, so the stylesheet can follow macOS's own settings, such as
// Increase Contrast, that a Windows contrast theme covers there.

if (navigator.userAgent.includes("Macintosh")) {
  document.documentElement.dataset.platform = "macos";
}
