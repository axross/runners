// what the page does with a token typed into an open form, run with
// `mise run test:app-frontend`. A small stand-in for the document is all the page's
// element builder needs, since no browser runs here.

import assert from "node:assert/strict";
import test from "node:test";

class Element {
  constructor(tag) {
    this.tag = tag;
    this.attributes = new Map();
    this.listeners = new Map();
    this.children = [];
    this.value = "";
    this.textContent = "";
  }
  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }
  getAttribute(name) {
    return this.attributes.get(name) ?? null;
  }
  removeAttribute(name) {
    this.attributes.delete(name);
  }
  addEventListener(type, listener) {
    const held = this.listeners.get(type) ?? [];
    held.push(listener);
    this.listeners.set(type, held);
  }
  dispatchEvent(event) {
    for (const listener of this.listeners.get(event.type) ?? []) {
      listener(event);
    }
    return true;
  }
  append(...nodes) {
    this.children.push(...nodes);
  }
  find(tag) {
    for (const child of this.children) {
      if (child.tag === tag) {
        return child;
      }
      const inside = child.find?.(tag);
      if (inside) {
        return inside;
      }
    }
    return null;
  }
}

globalThis.document = {
  createElement: (tag) => new Element(tag),
  createElementNS: (_namespace, tag) => new Element(tag),
  createTextNode: (text) => ({ text }),
};

const { clearTypedTokens, tokensPanel } =
  await import("../../frontend/tokens.js");

const ROW = {
  repository: "axross/runners",
  pools: "1 pool",
  state: "unset",
  state_text: "No token",
  status: null,
  controls: [],
  confirming_removal: false,
  form: {
    label: "Token",
    hint: "A fine-grained token.",
    error: null,
    save: "Save",
    cancel: "Cancel",
    save_reason_when_empty: "Type a token first.",
  },
};

function openForm() {
  const panel = tokensPanel({ intro: "Tokens.", rows: [ROW] }, false, {});
  const field = panel[1].find("input");
  const save = panel[1].find("button");
  return { field, save };
}

test("clearing the typed tokens empties an open field and leaves Save unavailable", () => {
  const { field, save } = openForm();
  field.value = "ghp_typed_and_not_sent";
  field.dispatchEvent({ type: "input" });
  assert.equal(save.getAttribute("aria-disabled"), "false");
  clearTypedTokens();
  assert.equal(field.value, "");
  assert.equal(save.getAttribute("aria-disabled"), "true");
});

test("the form stays open, so the field is still the one the panel draws", () => {
  const { field } = openForm();
  clearTypedTokens();
  const again = tokensPanel({ intro: "Tokens.", rows: [ROW] }, false, {});
  assert.equal(again[1].find("input"), field);
});
