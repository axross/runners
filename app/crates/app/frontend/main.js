const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

function show(view) {
  document.getElementById("status").textContent = view.tooltip;
}

invoke("read_view").then((shown) => show(shown.view));
listen("marshal:view", (event) => show(event.payload.view));

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    invoke("hide_window");
  }
});
