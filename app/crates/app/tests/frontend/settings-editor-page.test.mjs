// the Settings tab and the configuration editor as the main window keeps them apart, run with
// `mise run test:app-frontend`: each test loads its own copy of the page over a stand-in for
// the document and for the app's `invoke` and `listen`, so no browser and no app run here.
// the stand-in agent counts the revision of config.toml, which a saved setting moves.

import assert from "node:assert/strict";
import test from "node:test";

import { ConfigEditor } from "../../frontend/config-editor.js";
import { installDocument } from "./document.mjs";

const OPEN_REASON =
  "Settings cannot be changed while the configuration is open for editing. Apply, discard or close your changes first.";
const REVIEW_REASON =
  "Settings cannot be changed while a configuration change is being reviewed. Finish or cancel the review on the Pools tab first.";
const BUSY_REASON =
  "Settings cannot be changed while Marshal is reading, checking or applying the configuration.";
const SAVING_REASON = "Unavailable while a setting is being saved.";
const WAITING = "Waiting for the agent's answer.";
const VIEW_REASON =
  "Settings cannot be changed while Marshal's agent does not answer.";

const FILE = {
  status: "valid",
  settings: { routing_label: "axpc" },
  problems: [],
  pools: [{ name: "a", repository: "owner/a", runners: 2 }],
};

const CHANGE = { effects: [], repositories_released: [], applies: true };

const field = (key) => ({ key, kind: "text", label: key, absent: "Not set" });

const ROW = {
  key: "keep_awake",
  label: "Keep the computer awake while runners run",
  description: "Prevents idle sleep only.",
  state: "On (default).",
  checked: true,
  note: null,
};

/** the change a checkbox's `change` event sends in the model tests. */
const CHANGED = { key: "keep_awake", on: false };

let viewRevision = 0;
let pages = 0;

/** the view with the editor on offer, or without it, with the Settings tab's own reason. */
function shown({ editor = true, settingsReason = null, checked = true } = {}) {
  viewRevision += 1;
  return {
    revision: viewRevision,
    announce: [],
    view: {
      window: {
        ...(editor
          ? {
              editor: {
                stopped: false,
                settings: [field("routing_label")],
                pool: [field("name"), field("repository"), field("runners")],
                volume: [],
              },
            }
          : {}),
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
              ...FILE.pools[0],
              line: "Running",
              github_notes: [],
              runners: [],
              diagnostics: null,
            },
          ],
          tokens: { intro: "Tokens.", rows: [] },
          settings: {
            intro: "Saved in config.toml as soon as you change it.",
            unavailable: settingsReason,
            reason_in_intro: false,
            groups: [
              { title: "Sleep and network", rows: [{ ...ROW, checked }] },
            ],
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

/**
 * loads a fresh page over a stand-in agent. `agent.revision` is the revision of config.toml;
 * a `set_setting` call moves it and waits for `agent.holdSetting`, a `configuration` read
 * waits for `agent.holdRead` and answers `agent.file` or throws `agent.readError`.
 */
async function mount({ editor = true } = {}) {
  pages += 1;
  const elements = installDocument(["app", "live", "layer"]);
  const app = elements.get("app");
  const handlers = new Map();
  const agent = {
    revision: 1,
    file: FILE,
    readError: null,
    holdSetting: null,
    holdRead: null,
    holdCheck: null,
    checkError: null,
    settingError: null,
    calls: [],
    settingCalls: 0,
  };
  globalThis.window = {
    __TAURI__: {
      core: {
        invoke: async (command, args) => {
          if (command === "read_view") return shown({ editor });
          if (command === "set_setting") {
            agent.settingCalls += 1;
            agent.calls.push("set_setting");
            await agent.holdSetting;
            if (agent.settingError) throw agent.settingError;
            agent.revision += 1;
            return undefined;
          }
          if (command !== "configuration") return undefined;
          const { request } = args;
          agent.calls.push(
            request.kind === "read" ? "read" : `${request.kind}`,
          );
          if (request.kind === "read") {
            await agent.holdRead;
            if (agent.readError) throw agent.readError;
            return {
              kind: "read",
              value: { ...agent.file, revision: `rev${agent.revision}` },
            };
          }
          agent.requests = [...(agent.requests ?? []), request];
          await agent.holdCheck;
          if (agent.checkError) throw agent.checkError;
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
  await import(`../../frontend/main.js?page=${pages}`);
  const settle = () => new Promise((resolve) => setImmediate(resolve));
  const settled = async () => {
    for (let turn = 0; turn < 6; turn += 1) await settle();
  };
  const page = {
    agent,
    settled,
    byKey: (key) =>
      [...app.descendants()].find((element) => element.dataset.key === key),
    keysLike: (prefix) =>
      [...app.descendants()].filter((element) =>
        element.dataset.key?.startsWith(prefix),
      ),
    spoken: () =>
      elements.get("live").children.map((paragraph) => paragraph.textContent),
    /** sends a view as the app does, to the page's `marshal:view` listener. */
    send: (options) =>
      handlers.get("marshal:view")({ payload: shown(options) }),
    async press(key) {
      page.byKey(key).dispatchEvent({ type: "click" });
      await settled();
    },
    async type(key, value) {
      const input = page.byKey(key);
      input.value = value;
      input.dispatchEvent({ type: "input", target: input });
      await settled();
    },
    /** the accessible descriptions of a control, as the text a screen reader reads. */
    descriptions(key) {
      const ids = page.byKey(key).getAttribute("aria-describedby") ?? "";
      return ids
        .split(" ")
        .map(
          (id) =>
            [...app.descendants()].find(
              (element) => element.getAttribute("id") === id,
            )?.textContent,
        )
        .filter(Boolean);
    },
    /**
     * what a click on the setting's checkbox does in a browser: the click's default action
     * toggles it and fires `change` unless the page prevented it. returns whether it toggled.
     */
    clickSetting() {
      const box = page.byKey("setting:keep_awake");
      let prevented = false;
      box.dispatchEvent({
        type: "click",
        preventDefault: () => (prevented = true),
      });
      if (!prevented) {
        box.checked = !page.settingChecked();
        box.dispatchEvent({ type: "change" });
      }
      return !prevented;
    },
    /** whether the checkbox on show is checked: a drawn one holds the attribute until a click sets the state. */
    settingChecked() {
      const box = page.byKey("setting:keep_awake");
      return box.checked ?? box.getAttribute("checked") !== null;
    },
    settingIndeterminate: () =>
      page.byKey("setting:keep_awake").indeterminate === true,
    settingDisabled: () =>
      page.byKey("setting:keep_awake").getAttribute("aria-disabled") === "true",
    /** the page after its first view and the editor's first read. */
    async ready() {
      await settled();
      return page;
    },
    /** opens the first pool's form and changes its runners, which is an unapplied change. */
    async changeRunners(value = "3") {
      await page.press(page.keysLike("edit-pool:")[0].dataset.key);
      const runners = page
        .keysLike("config-field:")
        .find((element) => element.dataset.key.endsWith(":runners"));
      await page.type(runners.dataset.key, value);
    },
  };
  return page;
}

/** releases a promise a stand-in call waits on. */
function hold() {
  let release;
  const held = new Promise((resolve) => {
    release = resolve;
  });
  return { held, release };
}

test("a checkbox does not toggle while the editor holds unapplied changes", async () => {
  const page = await (await mount()).ready();
  assert.ok(page.byKey("setting:keep_awake"), "the Settings tab is drawn");
  await page.changeRunners();
  assert.equal(page.settingDisabled(), true, "the checkbox is not disabled");
  assert.equal(page.clickSetting(), false, "the click toggled the checkbox");
  assert.equal(page.agent.settingCalls, 0, "a setting was sent");
  assert.ok(
    page.descriptions("setting:keep_awake").includes(OPEN_REASON),
    "the reason is not among the checkbox's descriptions",
  );
});

test("the editor's controls are unavailable from a checkbox's change until its call returns", async () => {
  const page = await (await mount()).ready();
  const call = hold();
  page.agent.holdSetting = call.held;
  assert.equal(page.clickSetting(), true);
  await page.settled();
  assert.equal(page.agent.settingCalls, 1);
  for (const key of ["reload-config", "host-settings"]) {
    assert.equal(
      page.byKey(key).getAttribute("aria-disabled"),
      "true",
      `${key} is available while a setting is being saved`,
    );
    assert.deepEqual(page.descriptions(key), [SAVING_REASON]);
  }
  for (const key of [
    "add-pool",
    ...page.keysLike("edit-pool:").map((e) => e.dataset.key),
  ]) {
    assert.equal(
      page.byKey(key).getAttribute("aria-disabled"),
      "true",
      `${key} is available while a setting is being saved`,
    );
    assert.deepEqual(page.descriptions(key), [SAVING_REASON]);
  }
  call.release();
  await page.settled();
  for (const key of ["reload-config", "host-settings", "add-pool"]) {
    assert.equal(page.byKey(key).getAttribute("aria-disabled"), null);
  }
});

test("a changed checkbox keeps the value chosen from its change until its call returns", async () => {
  const page = await (await mount()).ready();
  const call = hold();
  page.agent.holdSetting = call.held;
  assert.equal(page.settingChecked(), true, "the row starts on");
  assert.equal(page.clickSetting(), true);
  // no turn has passed, and the agent has sent no view after the call
  assert.equal(
    page.settingChecked(),
    false,
    "the click's redraw undid the choice",
  );
  assert.equal(page.settingIndeterminate(), false);
  await page.settled();
  assert.equal(page.agent.settingCalls, 1);
  assert.equal(
    page.settingChecked(),
    false,
    "the choice was lost while the call was out",
  );
  call.release();
  await page.settled();
});

test("a redraw while a setting's call is out keeps the changed checkbox at the value chosen", async () => {
  const page = await (await mount()).ready();
  const call = hold();
  page.agent.holdSetting = call.held;
  assert.equal(page.clickSetting(), true);
  await page.settled();
  // the app has not yet sent a view with the setting being written
  await page.send({});
  await page.settled();
  assert.equal(
    page.settingChecked(),
    false,
    "a view without writing undid the choice",
  );
  await page.send({ settingsReason: VIEW_REASON });
  await page.settled();
  assert.equal(
    page.settingChecked(),
    false,
    "a view with a reason undid the choice",
  );
  assert.equal(page.settingIndeterminate(), false);
  call.release();
  await page.settled();
});

test("after a setting's call returns, the drawn checkbox follows the current view", async () => {
  const page = await (await mount()).ready();
  const call = hold();
  page.agent.holdSetting = call.held;
  assert.equal(page.clickSetting(), true);
  await page.settled();
  call.release();
  await page.settled();
  assert.equal(page.settingChecked(), true, "the view still says on");
  await page.send({ checked: false });
  await page.settled();
  assert.equal(page.settingChecked(), false, "a view that says off");
  await page.send({ checked: true });
  await page.settled();
  assert.equal(page.settingChecked(), true, "a view that says on");
  assert.equal(page.settingIndeterminate(), false);
});

test("a draft made after a setting was saved is sent at the revision the setting left", async () => {
  const page = await (await mount()).ready();
  assert.equal(page.clickSetting(), true);
  await page.settled();
  assert.equal(page.agent.revision, 2, "the setting moved the file's revision");
  await page.changeRunners();
  await page.press("check-config");
  const [request] = page.agent.requests;
  assert.equal(request.kind, "edit");
  assert.equal(request.revision, "rev2");
});

/** whether the setting's checkbox is disabled with `reason` as its description, and a click does not toggle it. */
function assertLocked(page, reason) {
  assert.equal(page.settingDisabled(), true, "the checkbox is not disabled");
  assert.ok(
    page.descriptions("setting:keep_awake").includes(reason),
    `the reason is not among the descriptions: ${page.descriptions("setting:keep_awake")}`,
  );
  const calls = page.agent.settingCalls;
  assert.equal(page.clickSetting(), false, "the click toggled the checkbox");
  assert.equal(page.agent.settingCalls, calls, "a setting was sent");
}

/** whether the checkbox is available, so that no reason is among its descriptions. */
function assertOpen(page) {
  assert.equal(page.settingDisabled(), false, "the checkbox is disabled");
  assert.equal(
    page.byKey("setting:keep_awake").getAttribute("aria-describedby").split(" ")
      .length,
    1,
  );
}

test("a pool's form that is shown makes the checkboxes unavailable until it closes", async () => {
  const page = await (await mount()).ready();
  assertOpen(page);
  await page.press(page.keysLike("edit-pool:")[0].dataset.key);
  assertLocked(page, OPEN_REASON);
  await page.press(page.keysLike("done-pool:")[0].dataset.key);
  assertOpen(page);
});

test("the Host Settings form that is shown makes the checkboxes unavailable until it closes", async () => {
  const page = await (await mount()).ready();
  await page.press("host-settings");
  assert.ok(
    page.keysLike("config-field:host:").length > 0,
    "the form is shown",
  );
  assertLocked(page, OPEN_REASON);
  await page.press("host-settings");
  assertOpen(page);
});

test("unapplied changes in a form that is closed keep the checkboxes unavailable until they are discarded", async () => {
  const page = await (await mount()).ready();
  await page.changeRunners();
  await page.press(page.keysLike("done-pool:")[0].dataset.key);
  assert.equal(page.keysLike("config-field:").length, 0, "the form is closed");
  assertLocked(page, OPEN_REASON);
  await page.press("discard-config");
  assertOpen(page);
});

test("a question about dropping changes keeps the checkboxes unavailable", async () => {
  const page = await (await mount()).ready();
  await page.changeRunners();
  await page.press("reload-config");
  assert.ok(page.byKey("reload-now"), "the question is open");
  assertLocked(page, OPEN_REASON);
  await page.press("keep-editing");
  assertLocked(page, OPEN_REASON);
  await page.press("discard-config");
  assertOpen(page);
});

test("an open review keeps the checkboxes unavailable until it is cancelled", async () => {
  const page = await (await mount()).ready();
  await page.press("reload-config");
  assert.ok(page.byKey("cancel-config"), "the review is open");
  assertLocked(page, REVIEW_REASON);
  await page.press("cancel-config");
  assertOpen(page);
});

test("a review of a draft names the review, and the draft's own reason returns when it is cancelled", async () => {
  const page = await (await mount()).ready();
  await page.changeRunners();
  await page.press("check-config");
  assertLocked(page, REVIEW_REASON);
  await page.press("cancel-config");
  assertLocked(page, OPEN_REASON);
});

test("a check in flight keeps the checkboxes unavailable until it answers", async () => {
  const page = await (await mount()).ready();
  const check = hold();
  page.agent.holdCheck = check.held;
  await page.changeRunners();
  await page.press("check-config");
  assertLocked(page, BUSY_REASON);
  check.release();
  await page.settled();
  assertLocked(page, REVIEW_REASON);
});

test("a read in flight keeps the checkboxes unavailable until it answers", async () => {
  const page = await (await mount()).ready();
  const read = hold();
  page.agent.holdRead = read.held;
  await page.press("reload-config");
  assertLocked(page, BUSY_REASON);
  read.release();
  await page.settled();
  assertLocked(page, REVIEW_REASON);
  await page.press("cancel-config");
  assertOpen(page);
});

test("a reason the view gives takes precedence over the editor's", async () => {
  const page = await (await mount()).ready();
  await page.changeRunners();
  page.send({ settingsReason: VIEW_REASON });
  await page.settled();
  assertLocked(page, VIEW_REASON);
  assert.equal(
    page.descriptions("setting:keep_awake").includes(OPEN_REASON),
    false,
  );
});

test("with no editor offered the checkboxes behave as they do without one", async () => {
  const page = await (await mount({ editor: false })).ready();
  assertOpen(page);
  assert.equal(page.clickSetting(), true);
  assert.equal(page.agent.settingCalls, 1);
});

/** a model with its file read, over a document-free stand-in, for what a page cannot reach. */
function editorModel() {
  const calls = [];
  const model = new ConfigEditor(
    async (_command, args) => {
      calls.push(args.request.kind);
      return { kind: "read", value: { ...FILE, revision: "r" } };
    },
    () => {},
    () => {},
  );
  model.update({
    stopped: false,
    settings: [field("routing_label")],
    pool: [field("name"), field("repository"), field("runners")],
    volume: [],
  });
  model.loaded({ ...FILE, revision: "r" });
  return { model, calls };
}

test("a form the editor holds but hides while the file is unusable is not shown, but unapplied changes in it still count", () => {
  const { model } = editorModel();
  model.loaded({ ...FILE, revision: "r", status: "missing" });
  assert.ok(model.fileReason, "the file is reported unusable");
  model.editing = 0;
  model.hostOpen = true;
  assert.equal(model.settingsReason, null);
});

test("unapplied changes in a hidden form make the checkboxes unavailable", () => {
  const { model } = editorModel();
  model.set(model.host, "routing_label", "other");
  model.error = { code: "config_file_form", message: "refused" };
  model.editing = 0;
  assert.ok(model.fileReason, "the form is hidden");
  assert.equal(model.settingsReason, OPEN_REASON);
  model.discard();
  assert.equal(model.settingsReason, null);
});

test("the editor offers no reason of its own when the agent offers no editor", () => {
  const { model } = editorModel();
  model.editing = 0;
  model.update(null);
  assert.equal(model.settingsReason, null);
});

test("a check or an apply does nothing while a setting is being saved", async () => {
  const { model, calls } = editorModel();
  model.set(model.host, "routing_label", "other");
  const call = hold();
  const saving = model.whileSettingSaves(CHANGED, () => call.held);
  assert.equal(model.saving, true);
  await model.check(true);
  await model.check();
  assert.deepEqual(calls, []);
  call.release();
  await saving;
  assert.equal(model.saving, false);
});

/** a model holding an unapplied review of a draft, with every request it sends recorded. */
async function reviewedModel() {
  const requests = [];
  const model = new ConfigEditor(
    async (_command, args) => {
      requests.push(args.request);
      return args.request.kind === "read"
        ? { kind: "read", value: { ...FILE, revision: "r" } }
        : { kind: "change", value: CHANGE };
    },
    () => {},
    () => {},
  );
  model.update({
    stopped: false,
    settings: [field("routing_label")],
    pool: [field("name"), field("repository"), field("runners")],
    volume: [],
  });
  model.loaded({ ...FILE, revision: "r" });
  model.set(model.host, "routing_label", "other");
  await model.check();
  assert.ok(model.review, "the draft is under review");
  requests.length = 0;
  return { model, requests };
}

test("the editor is locked while a setting is being saved, and by nothing else", async () => {
  const { model } = editorModel();
  assert.equal(model.locked, false);
  const call = hold();
  const saving = model.whileSettingSaves(CHANGED, () => call.held);
  assert.equal(model.locked, true);
  assert.equal(model.lockReason, SAVING_REASON);
  call.release();
  await saving;
  assert.equal(model.locked, false);
});

test("an apply of a reviewed draft sends nothing while a setting is being saved, and goes ahead once it is", async () => {
  const { model, requests } = await reviewedModel();
  const call = hold();
  const saving = model.whileSettingSaves(CHANGED, () => call.held);
  await model.apply();
  assert.deepEqual(requests, []);
  assert.ok(model.review, "the review is kept");
  assert.equal(model.busy, false);
  call.release();
  await saving;
  await model.apply();
  assert.deepEqual(
    requests.map((request) => [request.kind, request.check ?? null]),
    [
      ["edit", true],
      ["edit", false],
      ["read", null],
    ],
  );
});

test("two setting calls keep the editor held until both return", async () => {
  const { model } = editorModel();
  const first = hold();
  const second = hold();
  const one = model.whileSettingSaves(CHANGED, () => first.held);
  const two = model.whileSettingSaves(CHANGED, () => second.held);
  first.release();
  await one;
  assert.equal(model.saving, true);
  second.release();
  await two;
  assert.equal(model.saving, false);
});

/** the page after a setting call returned, with the editor's read from before it. */
async function afterSetting(options) {
  const page = await (await mount(options)).ready();
  assert.equal(page.clickSetting(), true);
  await page.settled();
  assert.equal(page.agent.revision, 2);
  page.agent.calls.length = 0;
  return page;
}

test("a pool's form opens only after a read that began once the setting call returned", async () => {
  const page = await afterSetting();
  const read = hold();
  page.agent.holdRead = read.held;
  await page.press(page.keysLike("edit-pool:")[0].dataset.key);
  assert.deepEqual(page.agent.calls, ["read"]);
  assert.equal(
    page.keysLike("config-field:").length,
    0,
    "the form opened before the read",
  );
  assert.deepEqual(page.descriptions("reload-config"), [WAITING]);
  read.release();
  await page.settled();
  assert.ok(page.keysLike("config-field:").length > 0, "the form did not open");
  assert.equal(page.spoken().at(-1), "config.toml read. Nothing was applied.");
});

/** the page with two pools of one name, as a hand edit can leave them, read and then stale. */
async function afterSettingWithTwins() {
  const page = await afterSetting();
  page.agent.file = {
    ...FILE,
    pools: [
      { name: "a", repository: "owner/first", runners: 2 },
      { name: "a", repository: "owner/second", runners: 3 },
    ],
  };
  await page.press(page.keysLike("edit-pool:")[0].dataset.key);
  await page.press(page.keysLike("done-pool:")[0].dataset.key);
  assert.equal(page.keysLike("edit-pool:").length, 2);
  assert.equal(page.clickSetting(), true);
  await page.settled();
  page.agent.calls.length = 0;
  return page;
}

/** the repository field of the form that is open. */
const openRepository = (page) =>
  page
    .keysLike("config-field:")
    .find((element) => element.dataset.key.endsWith(":repository"))?.value;

test("a stale read opens the form of the pool pressed among pools of one name", async () => {
  const page = await afterSettingWithTwins();
  await page.press(page.keysLike("edit-pool:")[1].dataset.key);
  assert.deepEqual(page.agent.calls, ["read"]);
  assert.equal(openRepository(page), "owner/second");
});

test("a stale read opens the one pool of the name when the pool pressed has moved", async () => {
  const page = await afterSettingWithTwins();
  page.agent.file = {
    ...FILE,
    pools: [{ name: "a", repository: "owner/second", runners: 3 }],
  };
  await page.press(page.keysLike("edit-pool:")[1].dataset.key);
  assert.equal(openRepository(page), "owner/second");
});

test("a stale read opens no form when several pools of the name remain and none is at the pool's place", async () => {
  const page = await afterSettingWithTwins();
  const second = page.keysLike("edit-pool:")[1].dataset.key;
  page.agent.file = {
    ...FILE,
    pools: [
      { name: "a", repository: "owner/x", runners: 1 },
      { name: "b", repository: "owner/y", runners: 1 },
      { name: "a", repository: "owner/z", runners: 1 },
    ],
  };
  await page.press(second);
  assert.equal(page.keysLike("config-field:").length, 0);
});

test("the new-pool form opens only after such a read", async () => {
  const page = await afterSetting();
  const read = hold();
  page.agent.holdRead = read.held;
  await page.press("add-pool");
  assert.deepEqual(page.agent.calls, ["read"]);
  assert.equal(
    page.keysLike("config-field:").length,
    0,
    "the form opened before the read",
  );
  read.release();
  await page.settled();
  const name = page
    .keysLike("config-field:")
    .find((element) => element.dataset.key.endsWith(":name"));
  assert.match(name.value, /^pool-\d+$/);
});

test("the Host Settings form opens only after such a read", async () => {
  const page = await afterSetting();
  const read = hold();
  page.agent.holdRead = read.held;
  await page.press("host-settings");
  assert.deepEqual(page.agent.calls, ["read"]);
  assert.equal(
    page.keysLike("config-field:").length,
    0,
    "the form opened before the read",
  );
  read.release();
  await page.settled();
  assert.ok(
    page.keysLike("config-field:host:").length > 0,
    "the form did not open",
  );
});

test("a read that ends the stale state is not repeated before the next form", async () => {
  const page = await afterSetting();
  await page.press("host-settings");
  await page.press("host-settings");
  await page.press(page.keysLike("edit-pool:")[0].dataset.key);
  await page.press(page.keysLike("done-pool:")[0].dataset.key);
  await page.press("add-pool");
  assert.deepEqual(page.agent.calls, ["read"]);
});

test("a setting the agent refused or did not answer also leaves the read stale", async () => {
  const page = await (await mount()).ready();
  page.agent.settingError = { code: "not_answered", message: "no answer" };
  assert.equal(page.clickSetting(), true);
  await page.settled();
  page.agent.calls.length = 0;
  await page.press(page.keysLike("edit-pool:")[0].dataset.key);
  assert.deepEqual(page.agent.calls, ["read"]);
  assert.ok(page.keysLike("config-field:").length > 0);
});

test("no form opens when the read finds the file unusable", async () => {
  const page = await afterSetting();
  page.agent.file = { ...FILE, status: "missing", pools: [] };
  await page.press(page.keysLike("edit-pool:")[0].dataset.key);
  assert.equal(page.keysLike("config-field:").length, 0);
  assert.equal(
    page.byKey("host-settings").getAttribute("aria-disabled"),
    "true",
  );
  assert.ok(page.byKey("read-config"), "the editor shows the read's result");
  assert.match(page.spoken().at(-1), /cannot be edited here/);
});

test("no form opens when the read fails, and the next try reads again", async () => {
  const page = await afterSetting();
  page.agent.readError = {
    code: "lost_contact",
    message: "Marshal is not answering.",
  };
  await page.press("add-pool");
  await page.press("host-settings");
  assert.equal(page.keysLike("config-field:").length, 0);
  page.agent.readError = null;
  await page.press("add-pool");
  assert.deepEqual(page.agent.calls, ["read", "read", "read"]);
  assert.ok(page.keysLike("config-field:").length > 0);
});

test("a read already out when the setting call returns leaves the editor stale", async () => {
  const page = await (await mount({ editor: false })).ready();
  const call = hold();
  const read = hold();
  page.agent.holdSetting = call.held;
  page.agent.holdRead = read.held;
  assert.equal(page.clickSetting(), true);
  page.send();
  await page.settled();
  assert.deepEqual(page.agent.calls, ["set_setting", "read"]);
  call.release();
  await page.settled();
  read.release();
  await page.settled();
  assert.ok(
    page.keysLike("edit-pool:").length > 0,
    "the first read did not finish",
  );
  await page.press(page.keysLike("edit-pool:")[0].dataset.key);
  assert.deepEqual(page.agent.calls, ["set_setting", "read", "read"]);
  assert.ok(page.keysLike("config-field:").length > 0);
});

test("the next Apply after a setting is sent at the revision the setting left, also from the Host Settings form", async () => {
  const page = await afterSetting();
  await page.press("host-settings");
  const label = page.keysLike("config-field:host:")[0];
  await page.type(label.dataset.key, "other");
  await page.press("check-config");
  const request = page.agent.requests.at(-1);
  assert.equal(request.kind, "edit");
  assert.equal(request.revision, "rev2");
});
