// the window's one polite live region, which exists from launch. Each message is its own
// paragraph, added in order; paragraphs the region has already spoken are dropped later.

import { h } from "./dom.js";

const KEPT = 20;

/** writes `messages`, in order, into the live region. */
export function say(messages) {
  const region = document.getElementById("live");
  if (!region) {
    return;
  }
  for (const message of messages) {
    region.append(h("p", { text: message }));
  }
  while (region.children.length > KEPT) {
    region.firstElementChild.remove();
  }
}
