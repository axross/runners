import assert from "node:assert/strict";
import test from "node:test";
import { ConfigEditor } from "../../frontend/config-editor.js";
import {
  editorFooter,
  editorNotice,
  editorPools,
  editorReview,
} from "../../frontend/editor.js";
import { h } from "../../frontend/dom.js";
import { Element, installDocument } from "./document.mjs";

const text = (key) => ({ key, kind: "text" });
const access = {
  stopped: false,
  settings: [text("routing_label")],
  pool: [
    text("name"),
    text("repository"),
    { key: "runners", kind: "integer" },
    { key: "labels", kind: "texts" },
    { key: "diagnostics", kind: "boolean" },
    { key: "memory_gb", kind: "integer" },
  ],
  volume: [text("suffix"), text("mount_path")],
};
const doc = {
  revision: "source",
  status: "valid",
  settings: { routing_label: "axpc" },
  problems: [],
  pools: [
    {
      name: "a",
      repository: "owner/a",
      runners: 2,
      volumes: [
        { suffix: "first", mount_path: "/a" },
        { suffix: "second", mount_path: "/b" },
      ],
    },
    { name: "b", repository: "owner/b", runners: 3 },
    { name: "c", repository: "owner/c", runners: 1 },
  ],
};

function editor(
  answer = () => ({
    kind: "change",
    value: {
      revision: "preview",
      effects: [],
      repositories_released: [],
      applies: true,
    },
  }),
) {
  const calls = [];
  const focuses = [];
  const announcements = [];
  const model = new ConfigEditor(
    async (command, args) => {
      calls.push({ command, ...args });
      return answer(command, args);
    },
    (focus) => {
      if (focus) focuses.push(focus);
    },
    (messages) => announcements.push(...messages),
  );
  model.update(access);
  model.loaded(structuredClone(doc));
  return { model, calls, focuses, announcements };
}

test("only changed keys are sent, retaining absent defaults, pool indices and volume comments", () => {
  const { model } = editor();
  assert.deepEqual(model.edits(), []);
  model.pools[0].volumes[0].removed = true;
  model.set(model.pools[0].volumes[1], "mount_path", "/changed");
  model.removePool(model.pools[1]);
  model.set(model.pools[2], "runners", "4");
  assert.deepEqual(model.edits(), [
    { op: "set", path: "pool[0].volumes[1].mount_path", value: "/changed" },
    { op: "remove_entry", path: "pool[0].volumes[0]" },
    { op: "set", path: "pool[2].runners", value: 4 },
    { op: "remove_entry", path: "pool[1]" },
  ]);
});

test("an invalid omitted optional field can be explicitly unset without writing missing defaults", () => {
  installDocument();
  const { model } = editor();
  model.editing = model.pools[0].id;
  const buttons = () =>
    editorPools(model, { pools: [] }, {}, () => null)
      .flatMap((node) => [...node.descendants()])
      .filter((node) => node.dataset.key?.startsWith("unset-config:"));
  assert.deepEqual(model.edits(), []);
  assert.equal(buttons().length, 0);
  model.loaded({
    ...structuredClone(doc),
    status: "invalid",
    problems: ["diagnostics", "memory_gb", "labels"].map((key) => ({
      path: `pool[0].${key}`,
      message: "must have the correct type",
    })),
  });
  model.editing = model.pools[0].id;
  assert.equal(model.dirty, false);
  for (const button of buttons()) button.dispatchEvent({ type: "click" });
  assert.equal(model.dirty, true);
  assert.deepEqual(model.edits(), [
    { op: "remove", path: "pool[0].labels" },
    { op: "remove", path: "pool[0].diagnostics" },
    { op: "remove", path: "pool[0].memory_gb" },
  ]);
  model.discard();
  assert.equal(model.dirty, false);
  assert.deepEqual(model.edits(), []);
});

test("a list field drops its empty lines and stays unset when every line is empty", () => {
  const { model } = editor();
  const pool = model.pools[0];
  model.set(pool, "labels", "one\ntwo\n");
  assert.deepEqual(model.edits(), [
    { op: "set", path: "pool[0].labels", value: ["one", "two"] },
  ]);
  model.set(pool, "labels", "one\n\ntwo");
  assert.deepEqual(model.edits(), [
    { op: "set", path: "pool[0].labels", value: ["one", "two"] },
  ]);
  const labelled = structuredClone(doc);
  labelled.pools[0].labels = ["kept"];
  model.loaded(labelled);
  model.set(model.pools[0], "labels", "\n");
  assert.deepEqual(model.edits(), [{ op: "remove", path: "pool[0].labels" }]);
});

test("a stopped confirmation expires on starting, including a check still in flight", async () => {
  let release;
  let held = false;
  const { model, calls } = editor(async () => {
    if (held) await new Promise((resolve) => (release = resolve));
    return {
      kind: "change",
      value: {
        applies: false,
        effects: [{ pool: "a", change: "restarted", stops_runners: false }],
        repositories_released: [],
      },
    };
  });
  model.update({ ...access, stopped: true });
  model.set(model.pools[0], "labels", "changed");
  await model.check();
  assert.equal(model.review.change.applies, false);
  model.update({ ...access, reason: "Starting" });
  model.update(access);
  await model.apply();
  assert.equal(model.review, null);
  assert.equal(
    calls.some((call) => call.request.check === false),
    false,
  );
  assert.equal(model.dirty, true);

  model.update({ ...access, stopped: true });
  held = true;
  const checking = model.check();
  model.update(access);
  release();
  await checking;
  assert.equal(model.review, null);
  assert.equal(
    calls.some((call) => call.request.check === false),
    false,
  );
  assert.match(model.message, /changed.*[Cc]heck/);
});

test("new runners require a fresh confirmation even when the file and running state stay the same", async () => {
  let hasRunners = false;
  const { model, calls, focuses } = editor((_command, { request }) =>
    request.kind === "read"
      ? { kind: "read", value: doc }
      : {
          kind: "change",
          value: {
            applies: true,
            effects: [
              { pool: "a", change: "restarted", stops_runners: hasRunners },
            ],
            repositories_released: [],
          },
        },
  );
  model.set(model.pools[0], "labels", "changed");
  await model.check();
  hasRunners = true;
  await model.apply();
  assert.equal(
    calls.some((call) => call.request.check === false),
    false,
  );
  assert.equal(model.review.change.effects[0].stops_runners, true);
  assert.equal(focuses.at(-1), "cancel-config");
  await model.apply();
  assert.equal(calls.filter((call) => call.request.check === false).length, 1);
});

test("the review names the failing jobs only for a pool whose effect stops runners", () => {
  installDocument();
  const { model } = editor();
  const effect = (pool, change, stops_runners) => ({
    pool,
    change,
    runners_before: 2,
    runners_after: 2,
    stops_runners,
  });
  model.review = {
    request: {},
    remove: new Set(),
    change: {
      applies: true,
      repositories_released: [],
      effects: [
        effect("restarting", "restarted", true),
        effect("idle-restart", "restarted", false),
        effect("gone", "removed", true),
        effect("gone-idle", "removed", false),
        effect("shrinking", "runners_removed", true),
      ],
    },
  };
  const textOf = (node) =>
    node.children
      .map((child) => (child instanceof Element ? textOf(child) : child.text))
      .join("")
      .concat(node.textContent);
  const lines = [...editorReview(model).descendants()]
    .filter((node) => node.getAttribute("class") === "description wrap")
    .map(textOf)
    .filter((line) => line.includes("("));
  assert.deepEqual(lines, [
    "restarting: Restarts with the new settings. Its running jobs fail. (2 → 2)",
    "idle-restart: Restarts with the new settings. (2 → 2)",
    "gone: Removed. Its running jobs fail. (2 → 2)",
    "gone-idle: Removed. (2 → 2)",
    "shrinking: Loses its highest-numbered slots. The other slots carry on. The jobs on the lost slots fail. (2 → 2)",
  ]);
});

test("check cannot write; cancel writes nothing; apply retains the source revision, not the preview", async () => {
  const { model, calls, focuses } = editor((command, args) =>
    args.request.kind === "read"
      ? { kind: "read", value: doc }
      : {
          kind: "change",
          value: {
            revision: "different-preview",
            effects: [],
            repositories_released: [],
            applies: true,
          },
        },
  );
  model.set(model.pools[0], "runners", "5");
  await model.check();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].request.check, true);
  assert.equal(focuses.at(-1), "cancel-config");
  assert.equal(model.cancel(), true);
  assert.equal(calls.length, 1);
  await model.check();
  assert.equal(model.review.remove.size, 0);
  await model.apply();
  assert.equal(
    calls.find((call) => call.request.check === false).request.revision,
    "source",
  );
  assert.equal(model.dirty, false);
});

test("a refusal and loss of contact retain the draft and field problems", async () => {
  const problem = {
    path: "pool[0].runners",
    line: 8,
    message: "must be at most 64",
  };
  const { model, calls } = editor(() => {
    throw {
      code: "config_invalid",
      message: "Invalid result",
      problems: [problem],
    };
  });
  model.set(model.pools[0], "runners", "80");
  await model.check();
  assert.deepEqual(model.error.problems, [problem]);
  assert.equal(model.pools[0].fields.runners, "80");
  model.update({ ...access, reason: "Lost contact" });
  await model.check();
  assert.equal(calls.length, 1);
  model.discard();
  assert.equal(model.pools[0].fields.runners, "2");
});

test("reload warns before dropping a draft, checks the new revision, and cancelling does not apply", async () => {
  const { model, calls } = editor((_command, args) =>
    args.request.kind === "read"
      ? { kind: "read", value: { ...doc, revision: "new-file" } }
      : {
          kind: "change",
          value: {
            revision: "new-file",
            effects: [],
            repositories_released: [],
            applies: true,
          },
        },
  );
  model.set(model.pools[0], "runners", "5");
  await model.check(true);
  assert.equal(calls.length, 0);
  assert.equal(model.reloadAsk, true);
  await model.check(true);
  assert.equal(calls[1].request.kind, "reload");
  assert.equal(calls[1].request.revision, "new-file");
  model.cancel();
  assert.equal(calls.length, 2);
});

test("a new pool with volumes is added after removals at its resulting index", () => {
  const { model, focuses } = editor();
  model.removePool(model.pools[0]);
  model.removePool(model.pools[2]);
  model.addPool();
  const pool = model.pools.at(-1);
  assert.equal(focuses.at(-1), `edit-heading:${pool.id}`);
  model.set(pool, "repository", "owner/new");
  model.addVolume(pool);
  model.set(pool.volumes[0], "suffix", "cache");
  model.set(pool.volumes[0], "mount_path", "/cache");
  assert.deepEqual(model.edits(), [
    { op: "remove_entry", path: "pool[2]" },
    { op: "remove_entry", path: "pool[0]" },
    {
      op: "add_entry",
      array: "pool",
      values: { name: pool.fields.name, repository: "owner/new", runners: 1 },
    },
    {
      op: "add_entry",
      array: "pool[1].volumes",
      values: { suffix: "cache", mount_path: "/cache" },
    },
  ]);
});

test("token removal is opt-in, a store failure leaves the configuration saved and retries only removal", async () => {
  installDocument();
  let refuseRemoval = true;
  const { model, calls, focuses, announcements } = editor(
    (_command, { request }) => {
      if (request.kind === "read") return { kind: "read", value: doc };
      if (request.kind === "remove_released_token") {
        if (refuseRemoval)
          throw { message: "Access refused (kind access_refused, code 5)" };
        return { kind: "token_removed" };
      }
      return {
        kind: "change",
        value: {
          effects: [],
          repositories_released: ["owner/released"],
          applies: true,
        },
      };
    },
  );
  const retry = () =>
    [...editorNotice(model).descendants()].find(
      (node) => node.dataset.key === "retry-token:owner/released",
    );
  model.set(model.pools[0], "runners", "4");
  await model.check();
  await model.apply();
  assert.equal(
    calls.some((call) => call.request.kind === "remove_released_token"),
    false,
  );
  model.set(model.pools[0], "runners", "4");
  await model.check();
  model.review.remove.add("owner/released");
  const focusCount = focuses.length;
  await model.apply();
  assert.equal(model.dirty, false);
  assert.equal(retry().getAttribute("aria-label"), "Try Again");
  assert.equal(focuses.length, focusCount);
  assert.ok(
    announcements.some((message) =>
      /not removed:.*access_refused/.test(message),
    ),
  );
  const writes = calls.filter((call) => call.request.check === false).length;
  refuseRemoval = false;
  await model.retryToken("owner/released");
  assert.equal(retry().getAttribute("aria-label"), "Removed");
  assert.equal(retry().getAttribute("aria-disabled"), "true");
  assert.match(
    announcements.at(-1),
    /stored token for owner\/released was removed/,
  );
  assert.equal(focuses.length, focusCount);
  assert.equal(
    calls.filter((call) => call.request.check === false).length,
    writes,
  );
});

test("a repository's return invalidates its old token-removal offer after read or save", async () => {
  installDocument();
  for (const [action, repository] of [
    ["read", "owner/released"],
    ["save", "Owner/Released"],
    ["save-unreadable", "Owner/Released"],
  ]) {
    let current = structuredClone(doc);
    let failRead = false;
    const { model, calls } = editor((_command, { request }) => {
      if (request.kind === "read") {
        if (failRead) throw { message: "Lost contact" };
        return { kind: "read", value: current };
      }
      if (request.kind === "remove_released_token")
        throw { message: "Access refused" };
      return {
        kind: "change",
        value: {
          effects: [],
          repositories_released: ["owner/released"],
          applies: true,
        },
      };
    });
    const retry = () =>
      [...editorNotice(model).descendants()].find(
        (node) => node.dataset.key === "retry-token:owner/released",
      );
    model.set(model.pools[0], "runners", "4");
    await model.check();
    model.review.remove.add("owner/released");
    await model.apply();
    const oldRetry = retry();
    assert.equal(oldRetry.getAttribute("aria-disabled"), null);

    current = {
      ...current,
      pools: [...current.pools, { name: "returned", repository, runners: 1 }],
    };
    if (action === "read") await model.read();
    else {
      model.addPool();
      model.set(model.pools.at(-1), "repository", repository);
      await model.check();
      failRead = action === "save-unreadable";
      await model.apply();
    }
    if (failRead) {
      assert.equal(retry().getAttribute("aria-label"), "Try Again");
      assert.equal(retry().getAttribute("aria-disabled"), "true");
      retry().dispatchEvent({ type: "click" });
      oldRetry.dispatchEvent({ type: "click" });
      await new Promise((resolve) => setImmediate(resolve));
      assert.equal(
        calls.filter((call) => call.request.kind === "remove_released_token")
          .length,
        1,
      );
      failRead = false;
      await model.read();
    }
    assert.equal(retry().getAttribute("aria-label"), "Cancelled");
    assert.equal(retry().getAttribute("aria-disabled"), "true");
    retry().dispatchEvent({ type: "click" });
    oldRetry.dispatchEvent({ type: "click" });
    current = structuredClone(doc);
    await model.read();
    assert.equal(retry().getAttribute("aria-label"), "Cancelled");
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(
      calls.filter((call) => call.request.kind === "remove_released_token")
        .length,
      1,
    );
  }
});

test("a successful write followed by a failed read is saved, not an unapplied draft", async () => {
  const { model, calls } = editor((_command, { request }) => {
    if (request.kind === "read") throw { message: "Lost contact" };
    return {
      kind: "change",
      value: { effects: [], repositories_released: [], applies: true },
    };
  });
  model.set(model.pools[0], "runners", "5");
  await model.check();
  await model.apply();
  assert.match(model.message, /Saved, but.*Lost contact/);
  assert.doesNotMatch(model.message, /unapplied changes are kept/);
  assert.equal(model.dirty, false);
  await model.check();
  assert.equal(calls.filter((call) => call.request.check === false).length, 1);
});

for (const code of ["not_answered", "lost_contact"]) {
  test(`a sent write that ends in ${code} is an unknown outcome, not a refusal`, async () => {
    installDocument();
    const { model, calls } = editor((_command, { request }) => {
      if (request.check === false) throw { code, message: "No answer." };
      return {
        kind: "change",
        value: { effects: [], repositories_released: [], applies: true },
      };
    });
    model.set(model.pools[0], "runners", "5");
    await model.check();
    await model.apply();
    assert.equal(
      calls.filter((call) => call.request.check === false).length,
      1,
    );
    assert.equal(model.applyResult, "Configuration may have been saved");
    assert.match(model.message, /may have changed.*Read config\.toml again/);
    assert.doesNotMatch(model.message, /not applied/);
    assert.equal(model.pools[0].fields.runners, "5");
    assert.equal(model.dirty, true);
    assert.equal(
      editorReview(model).getAttribute("aria-label"),
      "Configuration may have been saved",
    );
  });
}

test("a sent write the agent refuses is still not applied", async () => {
  const { model } = editor((_command, { request }) => {
    if (request.check === false)
      throw { code: "config_changed", message: "Changed." };
    return {
      kind: "change",
      value: { effects: [], repositories_released: [], applies: true },
    };
  });
  model.set(model.pools[0], "runners", "5");
  await model.check();
  await model.apply();
  assert.equal(model.applyResult, "Configuration not applied");
  assert.match(model.message, /Your unapplied changes are kept/);
});

test("a token removal that is not answered is neither removed nor failed, and offers no retry", async () => {
  installDocument();
  const { model, calls, announcements } = editor((_command, { request }) => {
    if (request.kind === "read") return { kind: "read", value: doc };
    if (request.kind === "remove_released_token")
      throw { code: "not_answered", message: "No answer." };
    return {
      kind: "change",
      value: {
        effects: [],
        repositories_released: ["owner/released"],
        applies: true,
      },
    };
  });
  model.set(model.pools[0], "runners", "4");
  await model.check();
  model.review.remove.add("owner/released");
  await model.apply();
  assert.equal(model.applyResult, "Configuration saved");
  assert.deepEqual(model.tokenRemovals.get("owner/released"), {
    state: "unanswered",
  });
  const row = [...editorNotice(model).descendants()].find(
    (node) => node.dataset.key === "retry-token:owner/released",
  );
  assert.equal(row.getAttribute("aria-label"), "Not Answered");
  assert.equal(row.getAttribute("aria-disabled"), "true");
  assert.ok(
    announcements.some((message) =>
      /has not answered about removing/.test(message),
    ),
  );
  assert.equal(
    [...editorNotice(model).descendants()].some((node) =>
      /not removed/.test(node.textContent ?? ""),
    ),
    false,
  );
  const removals = calls.filter(
    (call) => call.request.kind === "remove_released_token",
  ).length;
  await model.retryToken("owner/released");
  assert.equal(
    calls.filter((call) => call.request.kind === "remove_released_token")
      .length,
    removals,
  );
});

test("reading again asks before dropping an unapplied draft and can be cancelled", async () => {
  const { model, calls } = editor(() => ({ kind: "read", value: doc }));
  model.set(model.pools[0], "runners", "5");
  await model.read();
  assert.equal(model.readAsk, true);
  assert.equal(calls.length, 0);
  model.cancel();
  assert.equal(model.pools[0].fields.runners, "5");
  await model.read();
  await model.read(true);
  assert.equal(calls.length, 1);
  assert.equal(model.pools[0].fields.runners, "2");
});

test("field problems follow their source or result index after an earlier pool is removed", async () => {
  installDocument();
  let refuse = true;
  const { model, calls } = editor(() => {
    if (refuse)
      throw {
        code: "config_invalid",
        message: "Invalid result",
        problems: [{ path: "pool[1].runners", message: "must be at most 64" }],
      };
    return {
      kind: "change",
      value: { effects: [], repositories_released: [], applies: true },
    };
  });
  model.loaded({
    ...structuredClone(doc),
    status: "invalid",
    problems: [{ path: "pool[2].runners", message: "must be at most 64" }],
  });
  const pool = model.pools[2];
  model.removePool(model.pools[0]);
  model.editing = pool.id;
  const invalidField = () =>
    editorPools(model, { pools: [] }, {}, () => null)
      .flatMap((node) => [...node.descendants()])
      .find((node) => node.dataset.key === `config-field:${pool.id}:runners`)
      .getAttribute("aria-invalid");
  model.set(pool, "runners", "not-a-number");
  await model.check();
  assert.equal(calls.length, 0);
  assert.equal(model.error.problems[0].path, "pool[2].runners");
  assert.equal(invalidField(), "true");
  model.set(pool, "runners", "80");
  await model.check();
  assert.equal(calls.length, 1);
  assert.equal(model.error.problems[0].path, "pool[1].runners");
  assert.equal(invalidField(), "true");
  refuse = false;
  model.set(pool, "runners", "4");
  await model.check();
  assert.ok(model.review);
  assert.equal(invalidField(), null);
  assert.equal(
    [...editorNotice(model).descendants()].some((node) =>
      node.getAttribute("class")?.includes("config-error"),
    ),
    false,
  );
});

test("a removed pool remains visible until its runners leave the live snapshot", () => {
  installDocument();
  const { model } = editor();
  model.loaded({ ...doc, pools: [doc.pools[0]] });
  const list = { pools: [{ name: "a" }, { name: "stopping-pool" }] };
  const liveCard = (pool) =>
    h(
      "section",
      {},
      h("div", { class: "card-head" }, h("h2", { text: pool.name })),
    );
  const headings = () =>
    editorPools(model, list, {}, liveCard)
      .flatMap((node) => [...node.descendants()])
      .filter((node) => node.tag === "h2")
      .map((node) => node.textContent);
  assert.deepEqual(headings(), ["a", "stopping-pool"]);
  list.pools.pop();
  assert.deepEqual(headings(), ["a"]);
});

test("host settings explain an uneditable document without losing the initial read or editable forms", async () => {
  installDocument();
  const { model, calls } = editor(() => ({ kind: "read", value: doc }));
  const footer = () => editorFooter(model, "config.toml");
  const control = () =>
    [...footer().descendants()].find(
      (node) => node.dataset.key === "host-settings",
    );
  for (const value of [
    { ...doc, status: "missing" },
    { ...doc, status: "unreadable" },
    { ...doc, too_large: true },
  ]) {
    model.loaded(value);
    const button = control();
    assert.equal(button.getAttribute("aria-disabled"), "true");
    assert.ok(button.getAttribute("aria-describedby"));
    button.dispatchEvent({ type: "click" });
    assert.equal(model.hostOpen, false);
  }
  for (const status of ["valid", "invalid"]) {
    model.loaded({ ...doc, status });
    control().dispatchEvent({ type: "click" });
    assert.equal(model.hostOpen, true);
    assert.ok(
      [...footer().descendants()].some(
        (node) => node.getAttribute("aria-label") === "Host settings",
      ),
    );
  }
  model.document = null;
  model.hostOpen = false;
  control().dispatchEvent({ type: "click" });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(calls.length, 1);
  assert.equal(model.hostOpen, true);
});

test("the window edits in cards, keeps drafts across views and hides, labels problems, and cancels review with Escape", async () => {
  const elements = installDocument(["app", "live", "layer"]);
  const app = elements.get("app");
  const handlers = new Map();
  const requests = [];
  let revision = 0;
  let refuse = false;
  const fields = (specs) =>
    specs.map((spec) => ({ ...spec, label: spec.key, absent: "Not set" }));
  const shown = () => ({
    revision: ++revision,
    announce: [],
    view: {
      window: {
        editor: {
          stopped: false,
          settings: fields(access.settings),
          pool: fields(access.pool),
          volume: fields(access.volume),
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
          pools: doc.pools.map((pool) => ({
            ...pool,
            line: "Running",
            github_notes: [],
            runners: [],
            diagnostics: null,
          })),
          tokens: { rows: [] },
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
  });
  globalThis.window = {
    __TAURI__: {
      core: {
        invoke: async (command, args) => {
          if (command === "read_view") return shown();
          requests.push({ command, ...args });
          if (command !== "configuration") return;
          if (args.request.kind === "read") return { kind: "read", value: doc };
          if (refuse)
            throw {
              code: "config_invalid",
              message: "Invalid result",
              problems: [
                {
                  path: "pool[0].runners",
                  line: 9,
                  message: "must be at least 1",
                },
              ],
            };
          return {
            kind: "change",
            value: {
              effects: [
                {
                  pool: "a",
                  change: "restarted",
                  runners_before: 2,
                  runners_after: 2,
                  stops_runners: true,
                },
              ],
              repositories_released: [],
              applies: true,
            },
          };
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
  const poolsPanel = () =>
    [...app.descendants()].find(
      (element) => element.getAttribute("id") === "panel-pools",
    );
  const click = async (key) => {
    byKey(key).dispatchEvent({ type: "click" });
    await settle();
  };
  await settle();
  await click("tab:tokens");
  assert.equal(byKey("tab:tokens").getAttribute("aria-selected"), "true");
  await click("reload-config");
  assert.equal(poolsPanel().getAttribute("hidden"), null);
  assert.equal(document.activeElement, byKey("cancel-config"));
  document.dispatchEvent({
    type: "keydown",
    key: "Escape",
    preventDefault() {},
  });
  assert.equal(document.activeElement, byKey("reload-config"));
  const edit = [...app.descendants()].find((element) =>
    element.dataset.key?.startsWith("edit-pool:"),
  );
  const poolID = edit.dataset.key.split(":")[1];
  await click(edit.dataset.key);
  const key = `config-field:${poolID}:runners`;
  const input = byKey(key);
  const label = [...app.descendants()].find(
    (element) => element.getAttribute("for") === input.getAttribute("id"),
  );
  assert.equal(label.textContent, "runners");
  input.focus();
  input.value = "0";
  input.dispatchEvent({ type: "input", target: input });
  assert.equal(
    byKey(`fewer-runners:${poolID}`).getAttribute("aria-disabled"),
    "true",
  );
  await click(`fewer-runners:${poolID}`);
  assert.equal(byKey(key).value, "0");
  await click(`more-runners:${poolID}`);
  assert.equal(byKey(key).value, "1");
  await click(`fewer-runners:${poolID}`);
  assert.equal(byKey(key).value, "0");
  handlers.get("marshal:view")({ payload: shown() });
  handlers.get("marshal:hidden")();
  assert.equal(byKey(key).value, "0");
  assert.equal(document.activeElement, byKey(key));
  refuse = true;
  await click("check-config");
  assert.equal(byKey(key).getAttribute("aria-invalid"), "true");
  const description = [...app.descendants()].find(
    (element) =>
      element.getAttribute("id") ===
      byKey(key).getAttribute("aria-describedby"),
  );
  assert.match(
    description.textContent,
    /pool\[0\]\.runners.*line 9.*must be at least 1/,
  );
  assert.doesNotMatch(description.textContent, /Not set/);
  refuse = false;
  await click("tab:tokens");
  assert.equal(byKey("tab:tokens").getAttribute("aria-selected"), "true");
  await click("check-config");
  assert.equal(poolsPanel().getAttribute("hidden"), null);
  assert.equal(document.activeElement, byKey("cancel-config"));
  assert.equal(byKey(key).getAttribute("aria-disabled"), "true");
  assert.equal(
    byKey("confirm-config").getAttribute("class"),
    "pill destructive",
  );
  document.dispatchEvent({
    type: "keydown",
    key: "Escape",
    preventDefault() {},
  });
  assert.equal(document.activeElement, byKey("check-config"));
  assert.equal(byKey(key).value, "0");
  assert.equal(
    requests.filter((request) => request.request?.check === false).length,
    0,
  );
  assert.match(elements.get("live").children.at(-1).textContent, /cancelled/);
});
