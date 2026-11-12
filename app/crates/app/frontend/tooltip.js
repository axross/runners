// the balloon that says why a control is unavailable (WCAG 2.2 SC 1.4.13): it opens
// on hover and on keyboard focus, stays while the pointer is over the control, the
// balloon or the gap between them, closes on Esc, and stays inside the window.

import { h } from "./dom.js";
import { tail } from "./icons.js";

/** the room kept clear at the window's edge. */
const EDGE = 8;
const NARROW_WIDTH = 240;

const hovered = new Set();
const focused = new Set();
/** wrappers whose balloon Esc closed, until the pointer and focus have left them. */
const dismissed = new Set();
let current = null;

/** the vertical room a balloon has: the window, less what a scrolling ancestor cuts off. */
function roomAround(wrap) {
  let top = EDGE;
  let bottom = window.innerHeight - EDGE;
  for (let node = wrap.parentElement; node; node = node.parentElement) {
    if (/auto|scroll/.test(getComputedStyle(node).overflowY)) {
      const box = node.getBoundingClientRect();
      top = Math.max(top, box.top);
      bottom = Math.min(bottom, box.bottom);
    }
  }
  return { top, bottom };
}

function place(wrap, balloon) {
  const fitsSideways = () => {
    const box = balloon.getBoundingClientRect();
    return box.left >= EDGE && box.right <= window.innerWidth - EDGE;
  };
  if (!fitsSideways()) {
    balloon.classList.replace("end", "start");
  }
  if (!fitsSideways()) {
    // neither side has room for the full width, so narrow the balloon
    balloon.classList.add("narrow");
    if (wrap.getBoundingClientRect().right - NARROW_WIDTH >= EDGE) {
      balloon.classList.replace("start", "end");
    }
  }
  const room = roomAround(wrap);
  const below = balloon.getBoundingClientRect();
  if (below.bottom > room.bottom) {
    balloon.classList.add("above");
    if (balloon.getBoundingClientRect().top < room.top) {
      // it fits neither way; the side with less cut off wins
      const above = balloon.getBoundingClientRect();
      if (room.top - above.top > below.bottom - room.bottom) {
        balloon.classList.remove("above");
      }
    }
  }
}

function hide(wrap) {
  if (current?.wrap === wrap) {
    current.balloon.remove();
    current = null;
  }
}

function show(wrap, text) {
  if (current?.wrap === wrap) {
    return;
  }
  if (current) {
    hide(current.wrap);
  }
  const balloon = h(
    "div",
    { class: "tip end", role: "tooltip" },
    tail(),
    h("span", { text }),
  );
  wrap.append(balloon);
  place(wrap, balloon);
  current = { wrap, balloon };
}

function sync(wrap, reason) {
  const wanted = hovered.has(wrap) || focused.has(wrap);
  if (!wanted) {
    dismissed.delete(wrap);
  }
  const text = reason();
  if (wanted && text && !dismissed.has(wrap)) {
    show(wrap, text);
  } else {
    hide(wrap);
  }
}

/** makes `wrap` show `reason()` as a balloon while it has the pointer or focus. */
export function attachTip(wrap, reason) {
  const track = (set, add) => () => {
    if (add) {
      set.add(wrap);
    } else {
      set.delete(wrap);
    }
    sync(wrap, reason);
  };
  wrap.addEventListener("mouseenter", track(hovered, true));
  wrap.addEventListener("mouseleave", track(hovered, false));
  wrap.addEventListener("focusin", track(focused, true));
  wrap.addEventListener("focusout", track(focused, false));
}

/** closes the open balloon, as Esc does, and says whether there was one. */
export function closeTip() {
  if (!current) {
    return false;
  }
  dismissed.add(current.wrap);
  hide(current.wrap);
  return true;
}

/** forgets every balloon, before the window is drawn again. */
export function resetTips() {
  hovered.clear();
  focused.clear();
  dismissed.clear();
  current?.balloon.remove();
  current = null;
}
