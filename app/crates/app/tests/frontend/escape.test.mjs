// what Escape cancels on the main window, run with `mise run test:app-frontend`: the
// page is loaded as it is, over a stand-in for the document and for the app's `invoke`
// and `listen`, so no browser and no app run here.

import assert from "node:assert/strict";
import test from "node:test";

import { installDocument } from "./document.mjs";

const REPOSITORY = "axross/runners";

const DOC = {
  revision: "source",
  status: "valid",
  settings: { routing_label: "axpc" },
  problems: [],
  pools: [{ name: "a", repository: REPOSITORY, runners: 2 }],
};

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

const field = (key) => ({ key, kind: "text", label: key, absent: "Not set" });

let revision = 0;

/** the view with the token form open and the editor on offer. */
function shown() {
  revision += 1;
  return {
    revision,
    announce: [],
    view: {
      window: {
        editor: {
          stopped: false,
          settings: [field("routing_label")],
          pool: [field("name"), field("repository"), field("runners")],
          volume: [],
        },
        body: {
          kind: "list",
          header: {
            heading: "Marshal",
            line: "Running",
            controls: [],
            opens_on: "pools",
          },
          banners: [],
          pools: [
            {
              ...DOC.pools[0],
              line: "Running",
              github_notes: [],
              runners: [],
              diagnostics: null,
            },
          ],
          tokens: { intro: "Tokens.", rows: [ROW] },
          stale: false,
        },
        footer: {
          github: [],
          image_lines: [],
          warnings: [],
          configuration: "config.toml",
          note: "",
        },
      },
    },
  };
}

const app = installDocument(["app", "live", "layer"]).get("app");
const calls = [];
globalThis.window = {
  __TAURI__: {
    core: {
      invoke: async (command, args) => {
        calls.push({ command, ...args });
        if (command === "read_view") return shown();
        if (command !== "configuration") return undefined;
        if (args.request.kind === "read") return { kind: "read", value: DOC };
        return {
          kind: "change",
          value: {
            effects: [],
            repositories_released: [],
            applies: true,
          },
        };
      },
    },
    event: { listen: async () => () => {} },
  },
};
await import("../../frontend/main.js");

const settle = () => new Promise((resolve) => setImmediate(resolve));
const byKey = (key) =>
  [...app.descendants()].find((element) => element.dataset.key === key);
const press = async (key) => {
  byKey(key).dispatchEvent({ type: "click" });
  for (let turn = 0; turn < 4; turn += 1) await settle();
};
const escape = () =>
  document.dispatchEvent({
    type: "keydown",
    key: "Escape",
    preventDefault() {},
  });
const resultShown = () =>
  [...app.descendants()].some(
    (element) => element.getAttribute("aria-label") === "Configuration saved",
  );

test("Escape in a token form cancels the form, not the saved result the Pools tab holds", async () => {
  await settle();
  await press("reload-config");
  await press("confirm-config");
  assert.equal(resultShown(), true, "the saved result is on the Pools tab");
  await press("tab:tokens");
  const input = [...app.descendants()].find(
    (element) => element.tag === "input",
  );
  input.focus();
  calls.length = 0;
  escape();
  await settle();
  assert.deepEqual(
    calls.map(({ command, forms }) => ({ command, forms })),
    [{ command: "read_view", forms: { editing: null, removing: null } }],
    "the form is cancelled and the window stays",
  );
  assert.equal(resultShown(), true, "the saved result is still there");
});

test("Escape on the Tokens tab with no form open leaves the saved result and hides the window", async () => {
  const detached = document.createElement("div");
  detached.focus();
  calls.length = 0;
  escape();
  await settle();
  assert.deepEqual(
    calls.map(({ command }) => command),
    ["hide_window"],
  );
  assert.equal(resultShown(), true);
});

test("Escape on the Pools tab clears the saved result first", async () => {
  await press("tab:pools");
  calls.length = 0;
  escape();
  await settle();
  assert.equal(resultShown(), false);
  assert.deepEqual(calls, []);
});
