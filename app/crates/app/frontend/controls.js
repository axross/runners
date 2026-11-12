// a button for one of the view's controls. An unavailable control keeps the neutral
// look, stays focusable and is never dimmed: it reports itself disabled, its reason is
// its accessible description, and the same words open as a balloon (tooltip.js).

import { h } from "./dom.js";
import { icon } from "./icons.js";
import { attachTip } from "./tooltip.js";

let counter = 0;

/** an id for a description; the page makes up its ids, so none holds view text. */
export function nextId(prefix) {
  counter += 1;
  return `${prefix}-${counter}`;
}

/** the icon a header or banner button carries, by the control's action. */
const ICON_OF = {
  start_all: "play",
  stop_all: "square",
  stop_build: "square",
  build_image: "build",
  try_again: "retry",
};

/**
 * the key that finds a control again after the window is drawn again: the action's
 * name, with its repository for a token action.
 */
export function keyOf(action) {
  if (typeof action === "string") {
    return action;
  }
  const [[name, repository]] = Object.entries(action);
  return `${name}:${repository}`;
}

/** an unavailable control's reason as a hidden description and a balloon on `wrap`. */
export function describe(wrap, button, reason) {
  const id = nextId("why");
  button.setAttribute("aria-describedby", id);
  wrap.append(h("span", { id, class: "vh", text: reason }));
  attachTip(wrap, () => reason);
}

/**
 * the button for `spec`, a view `Control`.
 *
 * `shape` is "button" (44 px, with the action's icon) or "pill" (32 px, on a card or a
 * token row); `key` is the element's `data-key`; `name` is an accessible name that
 * puts the repository after the label, when the control has one.
 */
export function controlButton(spec, { shape, key, name, onPress }) {
  const unavailable = !spec.available;
  const iconName = shape === "button" ? ICON_OF[spec.action] : undefined;
  const button = h(
    "button",
    {
      type: "button",
      class: `${shape} ${spec.look}`,
      "data-key": key,
      "aria-label": name,
      "aria-disabled": unavailable ? "true" : null,
      on: {
        click: () => {
          if (!unavailable) {
            onPress(button);
          }
        },
      },
    },
    iconName ? icon(iconName, 20) : null,
    h("span", { text: spec.label }),
  );
  if (!unavailable || !spec.reason) {
    return button;
  }
  const wrap = h("span", { class: "tipwrap" }, button);
  describe(wrap, button, spec.reason);
  return wrap;
}
