// the Settings panel: the `[lifecycle]` settings in their groups, each a checkbox whose
// label toggles it, with its description and state as its accessible description and
// a note under it when the view gives one.
//
// a checkbox that cannot be changed stays focusable and is never dimmed: it reports
// itself disabled, the reason is part of its description and its balloon, and a click
// does not toggle it. the agent writes a change, and the next view shows what it came to.
// on macOS the first setting also shows its login item's state, with a button to Login
// Items while the state differs from the setting.

import { nextId } from "./controls.js";
import { h } from "./dom.js";
import { icon } from "./icons.js";
import { attachTip } from "./tooltip.js";

const NOTE_ICON = { info: "info", warning: "warning", error: "error" };

function note(row, noteId) {
  return h(
    "p",
    {
      id: noteId,
      class: `set-note description wrap${row.note.tone === "error" ? " bad" : ""}`,
    },
    icon(NOTE_ICON[row.note.tone] ?? "info", 20),
    h("span", { text: row.note.text }),
  );
}

function settingRow(row, off, ctx) {
  const offId = off?.id;
  const checkId = nextId("setting");
  const descriptionId = nextId("setting-description");
  const loginId = row.login_item ? nextId("setting-login") : null;
  const noteId = row.note ? nextId("setting-note") : null;
  // the balloon is made and removed on hover, so the reason stays on the page as well
  const fixedId = row.fixed ? nextId("setting-fixed") : null;
  const describedBy = [descriptionId, loginId, noteId, fixedId, offId]
    .filter(Boolean)
    .join(" ");
  const locked = Boolean(offId || fixedId);
  const box = h("input", {
    type: "checkbox",
    class: "check",
    id: checkId,
    checked: row.checked === true,
    "aria-describedby": describedBy,
    "aria-disabled": locked ? "true" : null,
    "data-key": `setting:${row.key}`,
    on: {
      click: (event) => {
        if (locked || row.checked === null) {
          event.preventDefault();
        }
      },
      change: () => ctx.setSetting(row.key, box.checked),
    },
  });
  // a value the view does not know shows neither on nor off
  box.indeterminate = row.checked === null;
  const control = locked ? h("span", { class: "tipwrap" }, box) : box;
  if (locked) {
    attachTip(control, () => (off ? off.reason : row.fixed));
  }
  return h(
    "div",
    { class: "set-row" },
    control,
    h(
      "div",
      { class: "set-text" },
      h("label", { for: checkId, class: "set-label", text: row.label }),
      h(
        "span",
        { id: descriptionId, class: "description low wrap" },
        `${row.description} `,
        h("span", { class: "set-state", text: row.state }),
      ),
      fixedId ? h("span", { id: fixedId, class: "vh", text: row.fixed }) : null,
    ),
    loginId
      ? h(
          "div",
          { id: loginId, class: "set-login description wrap" },
          "Login Items: ",
          h("b", { text: row.login_item }),
        )
      : null,
    row.note ? note(row, noteId) : null,
    row.open_login_items
      ? h("button", {
          type: "button",
          class: "button neutral set-button",
          "data-key": `setting:${row.key}:login-items`,
          text: row.open_login_items,
          on: { click: () => ctx.openLoginItems() },
        })
      : null,
  );
}

/** the Settings panel. */
export function settingsPanel(tab, ctx) {
  const offId = tab.unavailable ? nextId("settings-off") : null;
  // when the intro already says why, it is the reason every checkbox points to
  const introSaysWhy = tab.reason_in_intro;
  return [
    h("p", {
      id: introSaysWhy ? offId : null,
      class: `description wrap intro${introSaysWhy ? "" : " low"}`,
      text: tab.intro,
    }),
    tab.unavailable && !introSaysWhy
      ? h("p", {
          id: offId,
          class: "description wrap intro",
          text: tab.unavailable,
        })
      : null,
    tab.groups.map((group) => {
      const titleId = nextId("settings-group");
      return h(
        "div",
        { class: "set-group", role: "group", "aria-labelledby": titleId },
        h("h2", { class: "set-group-title", id: titleId, text: group.title }),
        h(
          "div",
          { class: "set-list" },
          group.rows.map((row) =>
            settingRow(
              row,
              offId && { id: offId, reason: tab.unavailable },
              ctx,
            ),
          ),
        ),
      );
    }),
  ];
}
