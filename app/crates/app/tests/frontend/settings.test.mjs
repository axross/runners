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
