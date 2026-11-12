// the question a stop asks before it runs, in the window: a modal over the whole window
// with focus on Cancel, Esc cancelling it, and focus going back to where it was.

import { nextId } from "./controls.js";
import { h } from "./dom.js";
import { icon } from "./icons.js";
import { closeTip } from "./tooltip.js";

let open = null;

export function dialogOpen() {
  return open !== null;
}

/** closes the question as a no, as Esc does. */
export function cancelDialog() {
  open?.close(false);
}

/**
 * asks `question` ({title, body, confirm, cancel}); `confirmed` runs on a yes, and
 * `returnFocus` runs once the question has closed, whichever the answer.
 */
export function askConfirmation(
  question,
  { iconName, confirmed, returnFocus },
) {
  if (open) {
    return;
  }
  closeTip();
  const app = document.getElementById("app");
  const titleId = nextId("dialog-title");
  const bodyId = nextId("dialog-body");

  const close = (accepted) => {
    open = null;
    scrim.remove();
    app.inert = false;
    returnFocus();
    if (accepted) {
      confirmed();
    }
  };

  const cancel = h("button", {
    type: "button",
    class: "dialog-button cancel",
    text: question.cancel,
    on: { click: () => close(false) },
  });
  const confirm = h(
    "button",
    {
      type: "button",
      class: "dialog-button confirm",
      on: { click: () => close(true) },
    },
    icon(iconName, 20),
    h("span", { text: question.confirm }),
  );
  const scrim = h(
    "div",
    { class: "scrim" },
    h(
      "div",
      {
        class: "dialog",
        role: "alertdialog",
        "aria-modal": "true",
        "aria-labelledby": titleId,
        "aria-describedby": bodyId,
        on: {
          keydown: (event) => {
            if (event.key === "Tab") {
              // two buttons: Tab and Shift+Tab swap between them
              event.preventDefault();
              (document.activeElement === cancel ? confirm : cancel).focus();
            }
          },
        },
      },
      h(
        "div",
        { class: "dialog-text" },
        h("h2", { id: titleId, class: "heading", text: question.title }),
        h("p", { id: bodyId, text: question.body }),
      ),
      h("div", { class: "dialog-actions" }, cancel, confirm),
    ),
  );
  document.getElementById("layer").append(scrim);
  app.inert = true;
  open = { close };
  cancel.focus();
}
