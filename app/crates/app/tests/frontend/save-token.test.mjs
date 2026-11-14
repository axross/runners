// what the main window's page does with a token it sends, run with
// `mise run test:app-frontend`: the page is loaded as it is, over a stand-in for the
// document and for the app's `invoke` and `listen`, so no browser and no app run here.

import assert from "node:assert/strict";
import test from "node:test";

import { Element, installDocument } from "./document.mjs";

const REPOSITORY = "axross/runners";
const TOKEN = "ghp_a_token_typed_into_the_field";

const ROW = {
  repository: REPOSITORY,
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

let revision = 0;

/** the reply to `read_view`, with the token form open. */
function shown() {
  revision += 1;
  return {
    revision,
    announce: [],
    view: {
      window: {
        body: {
          kind: "list",
          header: {
            heading: "Marshal",
            line: "No runners.",
            controls: [],
            tokens_chip: null,
            opens_on: "tokens",
          },
          banners: [],
          pools: [],
          pools_note: null,
          stale: false,
          tokens: { intro: "Tokens.", rows: [ROW] },
        },
        footer: {
          github: [],
          image_lines: [],
          warnings: [],
          configuration: "config.json",
          note: "A note.",
        },
      },
    },
  };
}

const { app, live } = Object.fromEntries(
  [...installDocument(["app", "live", "layer"])].map(([id, element]) => [
    id,
    element,
  ]),
);

/** every call the page made, and what each one answers; a test sets `setToken`. */
const calls = [];
const handlers = new Map();
let setToken = () => Promise.resolve();

globalThis.window = {
  __TAURI__: {
    core: {
      invoke: (command, args) => {
        calls.push({ command, args });
        return command === "set_token"
          ? setToken(args)
          : Promise.resolve(shown());
      },
    },
    event: {
      listen: (name, handler) => {
        handlers.set(name, handler);
        return Promise.resolve(() => {});
      },
    },
  },
};

await import("../../frontend/main.js");

/** lets the page's pending promises and timers run. */
async function settle() {
  for (let turn = 0; turn < 4; turn += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

function elements() {
  return [...app.descendants()];
}

function field() {
  return elements().find((element) => element.tag === "input");
}

function saveButton() {
  return elements().find(
    (element) => element.getAttribute("data-key") === `save:${REPOSITORY}`,
  );
}

/** types the token into the open field as a person does, and presses Save. */
function typeAndSave() {
  const input = field();
  input.value = TOKEN;
  input.dispatchEvent({ type: "input" });
  saveButton().dispatchEvent({ type: "click" });
  return input;
}

/** whether `text` is anywhere the page keeps or shows something: a value, an attribute or a text. */
function held(text) {
  const nodes = [...app.descendants(), ...live.descendants()];
  return nodes.some(
    (node) =>
      node.value.includes(text) ||
      node.textContent.includes(text) ||
      [...node.attributes.values()].some((value) => value.includes(text)) ||
      node.children.some(
        (child) => !(child instanceof Element) && child.text?.includes(text),
      ),
  );
}

/** the calls other than the one that carries the token, as text. */
function otherCalls() {
  return JSON.stringify(calls.filter((call) => call.command !== "set_token"));
}

await settle();

test("the page draws the view with the token form open", () => {
  assert.notEqual(field(), undefined);
  assert.notEqual(saveButton(), undefined);
});

const REFUSAL = "The token was not accepted.";

for (const [outcome, answer, shows] of [
  ["is stored", () => Promise.resolve(), false],
  ["is refused", () => Promise.reject(REFUSAL), true],
]) {
  test(`the field is empty and the page holds no token when the token ${outcome}`, async () => {
    calls.length = 0;
    let finish;
    setToken = () =>
      new Promise((resolve, reject) => {
        finish = () => answer().then(resolve, reject);
      });
    const input = typeAndSave();
    const sent = calls.find((call) => call.command === "set_token");
    assert.deepEqual(sent.args, { repository: REPOSITORY, token: TOKEN });
    assert.equal(input.value, "", "cleared while the app has not answered");
    finish();
    await settle();
    assert.equal(input.value, "");
    assert.equal(field(), input, "the form is still the one the page drew");
    assert.equal(saveButton().getAttribute("aria-disabled"), "true");
    assert.equal(
      held(REFUSAL),
      shows,
      "the refusal is shown only when refused",
    );
    assert.equal(held(TOKEN), false);
    assert.equal(otherCalls().includes(TOKEN), false);
    assert.equal(
      calls.filter((call) => call.command === "set_token").length,
      1,
    );
  });
}

test("a token half typed is cleared when the app says the window hides", async () => {
  const input = field();
  input.value = TOKEN;
  input.dispatchEvent({ type: "input" });
  assert.equal(saveButton().getAttribute("aria-disabled"), "false");
  handlers.get("marshal:hidden")({ payload: null });
  assert.equal(input.value, "");
  assert.equal(saveButton().getAttribute("aria-disabled"), "true");
  assert.equal(held(TOKEN), false);
});

test("a token half typed is also cleared when the web view reports the page hidden", () => {
  const input = field();
  input.value = TOKEN;
  document.hidden = true;
  document.dispatchEvent({ type: "visibilitychange" });
  document.hidden = false;
  assert.equal(input.value, "");
});
