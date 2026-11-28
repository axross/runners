// what the Settings panel does with a checkbox, run with `mise run test:app-frontend`:
// a change goes to the app once, and a checkbox that cannot be changed stays focusable
// with its reason in its description and does not toggle.

import assert from "node:assert/strict";
import test from "node:test";

import { installDocument } from "./document.mjs";

installDocument();

const { settingsPanel } = await import("../../frontend/settings.js");

const ROW = {
  key: "keep_awake",
  label: "Keep the computer awake while runners run",
  description: "Prevents idle sleep only.",
  state: "On (default).",
  checked: true,
  note: null,
};

function tab(changes = {}) {
  return {
    intro: "Saved in config.toml as soon as you change it.",
    unavailable: null,
    reason_in_intro: false,
    chip: null,
    groups: [{ title: "Sleep and network", rows: [ROW] }],
    ...changes,
  };
}

/** every element of the panel, in document order. */
function elements(panel) {
  return panel.flat().flatMap((node) => (node ? [...node.descendants()] : []));
}

function checkbox(panel) {
  return elements(panel).find((element) => element.tag === "input");
}

function byId(panel, id) {
  return elements(panel).find((element) => element.getAttribute("id") === id);
}

test("a change sends the setting's key and its new value", () => {
  const sent = [];
  const panel = settingsPanel(tab(), {
    setSetting: (key, on) => sent.push([key, on]),
  });
  const box = checkbox(panel);
  box.checked = false;
  box.dispatchEvent({ type: "change" });
  assert.deepEqual(sent, [["keep_awake", false]]);
});

test("an unavailable checkbox does not toggle and gives its reason in its description", () => {
  const why = "Settings cannot be changed while a setting is being saved.";
  const panel = settingsPanel(tab({ unavailable: why }), {});
  const box = checkbox(panel);
  assert.equal(box.getAttribute("aria-disabled"), "true");
  let prevented = false;
  box.dispatchEvent({
    type: "click",
    preventDefault: () => (prevented = true),
  });
  assert.ok(prevented);
  const described = box.getAttribute("aria-describedby").split(" ");
  assert.ok(
    described.some((id) => byId(panel, id)?.textContent === why),
    "the reason is not among the checkbox's descriptions",
  );
});

test("a reason the intro gives is the checkbox's description, not a second paragraph", () => {
  const why = "Settings cannot be changed while config.toml has problems.";
  const panel = settingsPanel(
    tab({ intro: why, unavailable: why, reason_in_intro: true }),
    {},
  );
  const paragraphs = elements(panel).filter(
    (element) => element.tag === "p" && element.textContent === why,
  );
  assert.equal(paragraphs.length, 1);
  const described = checkbox(panel).getAttribute("aria-describedby").split(" ");
  assert.ok(described.includes(paragraphs[0].getAttribute("id")));
});

test("a reason the intro does not give is its own paragraph the checkbox points to", () => {
  const why =
    "Settings cannot be changed while Marshal's agent does not answer.";
  const panel = settingsPanel(tab({ unavailable: why }), {});
  const paragraphs = elements(panel).filter(
    (element) => element.tag === "p" && element.textContent === why,
  );
  assert.equal(paragraphs.length, 1);
  const described = checkbox(panel).getAttribute("aria-describedby").split(" ");
  assert.ok(described.includes(paragraphs[0].getAttribute("id")));
});

test("the page follows reason_in_intro, not whether the intro and the reason are equal strings", () => {
  const why = "Settings cannot be changed while a setting is being saved.";
  const panel = settingsPanel(
    tab({ intro: why, unavailable: why, reason_in_intro: false }),
    {},
  );
  const paragraphs = elements(panel).filter(
    (element) => element.tag === "p" && element.textContent === why,
  );
  assert.equal(paragraphs.length, 2);
  const [intro, reason] = paragraphs;
  const described = checkbox(panel).getAttribute("aria-describedby").split(" ");
  assert.ok(described.includes(reason.getAttribute("id")));
  assert.ok(!described.includes(intro.getAttribute("id")));
});

test("a value the view does not know shows neither on nor off and does not toggle", () => {
  const panel = settingsPanel(
    tab({ groups: [{ title: "Sleep", rows: [{ ...ROW, checked: null }] }] }),
    {},
  );
  const box = checkbox(panel);
  assert.equal(box.indeterminate, true);
  assert.equal(box.getAttribute("checked"), null);
  let prevented = false;
  box.dispatchEvent({
    type: "click",
    preventDefault: () => (prevented = true),
  });
  assert.ok(prevented);
});

test("a note under a setting is part of its description", () => {
  const note = { text: "Not saved: config.toml changed.", tone: "error" };
  const panel = settingsPanel(
    tab({ groups: [{ title: "Sleep", rows: [{ ...ROW, note }] }] }),
    {},
  );
  const described = checkbox(panel).getAttribute("aria-describedby").split(" ");
  const noteElement = described
    .map((id) => byId(panel, id))
    .find((element) => element?.tag === "p");
  assert.ok(noteElement, "the note is not among the checkbox's descriptions");
  assert.match(noteElement.getAttribute("class"), /\bbad\b/);
});

const REASON =
  "macOS always stops the runners when the primary network changes.";

test("a fixed checkbox keeps its reason on the page, does not toggle and sends nothing", () => {
  const sent = [];
  const fixed = { ...ROW, key: "stop_on_network_change", fixed: REASON };
  const panel = settingsPanel(
    tab({ groups: [{ title: "Sleep and network", rows: [fixed] }] }),
    { setSetting: (key, on) => sent.push([key, on]) },
  );
  const box = checkbox(panel);
  assert.equal(box.getAttribute("aria-disabled"), "true");
  let prevented = false;
  box.dispatchEvent({
    type: "click",
    preventDefault: () => (prevented = true),
  });
  assert.ok(prevented);
  assert.equal(
    box.parent.getAttribute("class"),
    "tipwrap",
    "the balloon's wrapper",
  );
  const reason = box
    .getAttribute("aria-describedby")
    .split(" ")
    .map((id) => byId(panel, id))
    .find((element) => element?.textContent === REASON);
  assert.ok(reason, "the reason is not among the checkbox's descriptions");
  assert.equal(reason.getAttribute("class"), "vh");
  assert.deepEqual(sent, []);
});

test("the login item's line is part of the description and its button opens Login Items", () => {
  let opened = 0;
  const first = {
    ...ROW,
    key: "start_at_login",
    login_item: "Waiting for approval",
    note: { text: "macOS needs your approval.", tone: "warning" },
    open_login_items: "Open Login Items Settings",
  };
  const panel = settingsPanel(
    tab({ groups: [{ title: "Starting Marshal", rows: [first] }] }),
    { openLoginItems: () => (opened += 1) },
  );
  const described = checkbox(panel)
    .getAttribute("aria-describedby")
    .split(" ")
    .map((id) => byId(panel, id));
  const line = described.find((element) =>
    element?.getAttribute("class")?.includes("set-login"),
  );
  assert.ok(
    line,
    "the login item's line is not among the checkbox's descriptions",
  );
  assert.equal(line.find("b").textContent, "Waiting for approval");
  const button = elements(panel).find((element) => element.tag === "button");
  assert.equal(button.textContent, "Open Login Items Settings");
  button.dispatchEvent({ type: "click" });
  assert.equal(opened, 1);
});

test("a row without a login item has no line and no button", () => {
  const panel = settingsPanel(tab(), {});
  assert.equal(
    elements(panel).find((element) => element.tag === "button"),
    undefined,
  );
  assert.equal(
    elements(panel).find((element) =>
      element.getAttribute("class")?.includes("set-login"),
    ),
    undefined,
  );
});
