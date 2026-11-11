// the menu's confirmation window, which opens on its own when the main window is hidden.
// Focus starts on Cancel, and Esc and the window's close button answer no.

import { h } from "./dom.js";
import { icon } from "./icons.js";

const { invoke } = window.__TAURI__.core;

function answer(accepted) {
  invoke("answer_confirmation", { accepted });
}

function show(question) {
  const cancel = h("button", {
    type: "button",
    class: "dialog-button cancel",
    text: question.cancel,
    on: { click: () => answer(false) },
  });
  const confirm = h(
    "button",
    {
      type: "button",
      class: "dialog-button confirm",
      on: { click: () => answer(true) },
    },
    icon("square", 20),
    h("span", { text: question.confirm }),
  );
  document.getElementById("own").replaceChildren(
    h(
      "div",
      {
        class: "dialog",
        role: "alertdialog",
        "aria-labelledby": "title",
        "aria-describedby": "body",
        on: {
          keydown: (event) => {
            if (event.key === "Tab") {
              event.preventDefault();
              (document.activeElement === cancel ? confirm : cancel).focus();
            }
          },
        },
      },
      h(
        "div",
        { class: "dialog-text" },
        h("h1", { id: "title", class: "heading", text: question.title }),
        h("p", { id: "body", text: question.body }),
      ),
      h("div", { class: "dialog-actions" }, cancel, confirm),
    ),
  );
  cancel.focus();
}

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    answer(false);
  }
});

invoke("read_confirmation").then(show, (error) => {
  document
    .getElementById("own")
    .replaceChildren(
      h("p", { class: "description", role: "alert", text: String(error) }),
    );
});
