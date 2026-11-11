const { invoke } = window.__TAURI__.core;

function answer(accepted) {
  invoke("answer_confirmation", { accepted });
}

invoke("read_confirmation").then((question) => {
  document.getElementById("title").textContent = question.title;
  document.getElementById("body").textContent = question.body;
  document.getElementById("confirm").textContent = question.confirm;
  document.getElementById("cancel").textContent = question.cancel;
  document.getElementById("cancel").focus();
});

document.getElementById("confirm").addEventListener("click", () => answer(true));
document.getElementById("cancel").addEventListener("click", () => answer(false));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    answer(false);
  }
});
