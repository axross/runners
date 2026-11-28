// the configuration review as the main window drives it, run with `mise run test:app-frontend`:
// the page is loaded as it is, over a stand-in for the document and for the app's `invoke`
// and `listen`, so no browser and no app run here. An answer can be held until released.

import assert from "node:assert/strict";
import test from "node:test";

import { installDocument } from "./document.mjs";

const APPLYING = "Marshal is applying the last change.";

const DOC = {
  revision: "source",
  status: "valid",
  settings: { routing_label: "axpc" },
  problems: [],
  pools: [{ name: "a", repository: "owner/a", runners: 2 }],
};

const CHANGE = {
  effects: [],
  repositories_released: [],
  applies: true,
};

const field = (key) => ({ key, kind: "text", label: key, absent: "Not set" });

let revision = 0;

/** the view with the editor on offer, or refusing for `reason`. */
function shown(reason) {
  revision += 1;
  return {
    revision,
    announce: [],
    view: {
      window: {
        editor: {
          stopped: false,
          ...(reason ? { reason } : {}),
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
          tokens: { intro: "Tokens.", rows: [] },
          settings: {
            intro: "Settings.",
            unavailable: null,
            reason_in_intro: false,
            groups: [],
            chip: null,
          },
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

const elements = installDocument(["app", "live", "layer"]);
const app = elements.get("app");
const handlers = new Map();
let hold = null;

globalThis.window = {
  __TAURI__: {
    core: {
      invoke: async (command, args) => {
        if (command === "read_view") return shown();
        if (command !== "configuration") return undefined;
        if (args.request.kind === "read") return { kind: "read", value: DOC };
        await hold?.(args.request);
        return { kind: "change", value: CHANGE };
      },
    },
    event: {
      listen: async (name, handler) => {
        handlers.set(name, handler);
        return () => {};
      },
    },
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
const spoken = () =>
  elements.get("live").children.map((paragraph) => paragraph.textContent);

/** holds every request `matches` accepts, until the returned function releases them. */
function holding(matches) {
  let release;
  const released = new Promise((resolve) => {
    release = resolve;
  });
  hold = (request) => (matches(request) ? released : undefined);
  return release;
}

test("a view that refuses edits while the confirmed write is outstanding leaves the review and its focus", async () => {
  await settle();
  await press("reload-config");
  assert.ok(byKey("confirm-config"), "the check opened a review");
  const release = holding((request) => request.check === false);
  byKey("confirm-config").focus();
  await press("confirm-config");
  handlers.get("marshal:view")({ payload: shown(APPLYING) });
  await settle();
  assert.equal(document.activeElement, byKey("confirm-config"));
  release();
  for (let turn = 0; turn < 4; turn += 1) await settle();
  assert.equal(byKey("confirm-config").getAttribute("aria-label"), "Done");
  assert.equal(document.activeElement, byKey("confirm-config"));
  assert.equal(
    spoken().some((message) => /state changed/.test(message)),
    false,
  );
  assert.match(spoken().at(-1), /^Saved\./);
});

/** the reason an unavailable control gives, as its accessible description. */
function reasonOf(key) {
  const id = byKey(key).getAttribute("aria-describedby");
  return [...app.descendants()].find(
    (element) => element.getAttribute("id") === id,
  )?.textContent;
}

test("while a check is in flight the locked controls say they wait for the agent, then point to the review", async () => {
  handlers.get("marshal:view")({ payload: shown() });
  document.dispatchEvent({
    type: "keydown",
    key: "Escape",
    preventDefault() {},
  });
  const edit = [...app.descendants()].find((element) =>
    element.dataset.key?.startsWith("edit-pool:"),
  );
  await press(edit.dataset.key);
  const id = edit.dataset.key.split(":")[1];
  const input = byKey(`config-field:${id}:runners`);
  input.value = "3";
  input.dispatchEvent({ type: "input", target: input });
  const release = holding((request) => request.check === true);
  await press("check-config");
  const waiting = "Waiting for the agent's answer.";
  assert.equal(reasonOf(`remove-pool:${id}`), waiting);
  assert.equal(reasonOf("reload-config"), waiting);
  assert.equal(reasonOf("host-settings"), waiting);
  assert.equal(reasonOf("discard-config"), waiting);
  assert.equal(
    byKey(`config-field:${id}:runners`).getAttribute("aria-disabled"),
    "true",
  );
  release();
  for (let turn = 0; turn < 4; turn += 1) await settle();
  const review = "Finish or cancel the review first.";
  assert.equal(reasonOf(`remove-pool:${id}`), review);
  assert.equal(reasonOf("reload-config"), review);
  assert.equal(reasonOf("host-settings"), review);
});
