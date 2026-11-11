window.__TAURI__.core.invoke("read_view").then((view) => {
  document.getElementById("status").textContent = view.tooltip;
});
