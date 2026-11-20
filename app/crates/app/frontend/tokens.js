// the Tokens panel: a row per repository with its state and actions, the masked field
// that opens in the row, and the row's own question before a token is removed.
//
// The field is the one element that outlives a redraw, so that what is typed in it is
// kept while the view changes. The page holds a token nowhere else: it is read from the
// field only to pass to the app, and the field is cleared at once.

import { controlButton, describe, nextId } from "./controls.js";
import { h } from "./dom.js";
import { icon } from "./icons.js";

/** the open fields by repository, and what keeps each one's Save button in step. */
const fields = new Map();
const saves = new Map();

export function tokenKey(row) {
  return row.organization ?? row.repository;
}

export function tokenRows(tab) {
  return [...tab.rows, ...(tab.organizations ?? [])];
}

function fieldFor(repository, hintId, errorId) {
  let field = fields.get(repository);
  if (!field) {
    field = h("input", {
      type: "password",
      class: "field",
      autocomplete: "off",
      spellcheck: "false",
      autocapitalize: "off",
      autocorrect: "off",
      "data-key": `field:${repository}`,
    });
    field.addEventListener("input", () => saves.get(repository)?.());
    fields.set(repository, field);
  }
  field.setAttribute(
    "aria-describedby",
    errorId ? `${hintId} ${errorId}` : hintId,
  );
  return field;
}

/** drops the fields of rows that no longer show one, clearing them first. */
export function forgetClosedFields(rows) {
  const open = new Set(rows.filter((row) => row.form).map(tokenKey));
  for (const [repository, field] of fields) {
    if (!open.has(repository)) {
      field.value = "";
      fields.delete(repository);
      saves.delete(repository);
    }
  }
}

/**
 * empties every open field, keeping the forms open, for when the window hides: a token
 * half typed is not left in a page nobody can see.
 */
export function clearTypedTokens() {
  for (const field of fields.values()) {
    field.value = "";
    field.dispatchEvent(new Event("input"));
  }
}

/** Save is unavailable while the field is empty, and says why. */
function saveButton(form, repository, field, ctx) {
  const button = h(
    "button",
    {
      type: "button",
      class: "button solid",
      "data-key": `save:${repository}`,
      on: {
        click: () =>
          button.getAttribute("aria-disabled") !== "true" &&
          ctx.saveToken(repository, field),
      },
    },
    h("span", { text: form.save }),
  );
  const wrap = h("span", { class: "tipwrap" }, button);
  describe(wrap, button, form.save_reason_when_empty);
  const reasonId = button.getAttribute("aria-describedby");
  const sync = () => {
    const empty = field.value === "";
    button.setAttribute("aria-disabled", String(empty));
    if (empty) {
      button.setAttribute("aria-describedby", reasonId);
    } else {
      button.removeAttribute("aria-describedby");
    }
  };
  saves.set(repository, sync);
  sync();
  return wrap;
}

function tokenForm(row, ctx) {
  const form = row.form;
  const repository = tokenKey(row);
  const fieldId = nextId("token");
  const hintId = nextId("hint");
  const errorId = form.error ? nextId("error") : null;
  const field = fieldFor(repository, hintId, errorId);
  field.id = fieldId;
  if (form.error) {
    field.setAttribute("aria-invalid", "true");
  } else {
    field.removeAttribute("aria-invalid");
  }
  field.onkeydown = (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      if (field.value !== "") {
        ctx.saveToken(repository, field);
      }
    }
  };
  return h(
    "form",
    { class: "token-form", on: { submit: (event) => event.preventDefault() } },
    h("label", { for: fieldId, text: form.label }),
    field,
    h("p", { id: hintId, class: "caption low wrap hint", text: form.hint }),
    form.error
      ? h(
          "div",
          { class: "field-error" },
          icon("error", 20),
          h("span", {
            id: errorId,
            class: "description wrap",
            text: form.error,
          }),
        )
      : null,
    h(
      "div",
      { class: "form-actions" },
      saveButton(form, repository, field, ctx),
      h("button", {
        type: "button",
        class: "button neutral",
        "data-key": `cancel:${repository}`,
        text: form.cancel,
        on: { click: () => ctx.cancelForm(repository) },
      }),
    ),
  );
}

/** the row's own question before a token is removed; Cancel comes first. */
function removalQuestion(row, ctx) {
  const repository = tokenKey(row);
  const question = row.controls.find(
    (spec) => typeof spec.action === "object" && "remove_token" in spec.action,
  )?.confirmation;
  if (!question) {
    return null;
  }
  const titleId = nextId("remove-title");
  const bodyId = nextId("remove-body");
  return h(
    "div",
    {
      class: "token-confirm",
      role: "group",
      "aria-labelledby": titleId,
      "aria-describedby": bodyId,
    },
    h("p", { id: titleId, class: "label", text: question.title }),
    h("p", { id: bodyId, class: "description low wrap", text: question.body }),
    h(
      "div",
      { class: "form-actions" },
      h("button", {
        type: "button",
        class: "button neutral",
        "data-key": `cancel-remove:${repository}`,
        text: question.cancel,
        on: { click: () => ctx.cancelRemoval(repository) },
      }),
      h(
        "button",
        {
          type: "button",
          class: "button destructive",
          "data-key": `confirm-remove:${repository}`,
          on: { click: () => ctx.removeToken(repository) },
        },
        icon("trash", 20),
        h("span", { text: question.confirm }),
      ),
    ),
  );
}

function tokenRow(row, ctx) {
  const repository = tokenKey(row);
  const asking = row.form || row.confirming_removal;
  const controls = asking
    ? []
    : row.controls.map((spec, index) =>
        controlButton(spec, {
          shape: "pill",
          key: `row:${repository}:${index === 0 ? "primary" : "remove"}`,
          name: `${spec.label} for ${row.organization ? "organisation " : ""}${repository}`,
          onPress: (button) => ctx.press(spec, button),
        }),
      );
  return h(
    "li",
    {
      class: "token-row",
      "data-token-key": repository,
    },
    h(
      "div",
      { class: "token-line" },
      h(
        "div",
        { class: "r" },
        h("p", { class: "row-label wrap", text: repository }),
        h("p", { class: "caption low wrap", text: row.pools }),
      ),
      h(
        "div",
        { class: "s" },
        icon(row.state === "set" ? "key" : "warning", 20),
        h("span", {
          class: `description wrap${row.state === "set" ? " low" : ""}`,
          text: row.state_text,
        }),
      ),
      h("div", { class: "a" }, controls),
    ),
    row.status
      ? h("p", { class: "caption wrap token-status", text: row.status })
      : null,
    row.form ? tokenForm(row, ctx) : null,
    row.confirming_removal ? removalQuestion(row, ctx) : null,
  );
}

/** the Tokens panel. */
export function tokensPanel(tab, stale, ctx) {
  forgetClosedFields(tokenRows(tab));
  if (tab.organizations?.length) {
    return [
      h("p", { class: "description low wrap intro", text: tab.intro }),
      tokenSection(
        "Repositories",
        "a token for one repository, with Administration: read and write",
        tab.rows,
        stale,
        ctx,
      ),
      tokenSection(
        "Organisations",
        "a token for organisation runners, with Self-hosted runners: read and write",
        tab.organizations,
        stale,
        ctx,
      ),
    ];
  }
  return [
    h("p", { class: "description low wrap intro", text: tab.intro }),
    h(
      "section",
      { class: `token-list${stale ? " stale" : ""}`, "aria-label": "Tokens" },
      h(
        "ul",
        {},
        tab.rows.map((row) => tokenRow(row, ctx)),
      ),
    ),
  ];
}

/** a heading belongs outside the list so its rows retain the same layout in both sections. */
function tokenSection(title, permission, rows, stale, ctx) {
  const headingId = nextId("token-section");
  return h(
    "section",
    { class: "panel token-section", "aria-labelledby": headingId },
    h(
      "h2",
      { id: headingId, class: "token-heading" },
      h("span", { class: "row-label", text: title }),
      h("span", {
        class: "caption low wrap token-permission",
        "aria-hidden": "true",
        text: permission,
      }),
      h("span", { class: "vh", text: `: ${permission}` }),
    ),
    h(
      "div",
      { class: `token-list${stale ? " stale" : ""}` },
      h(
        "ul",
        {},
        rows.map((row) => tokenRow(row, ctx)),
      ),
    ),
  );
}
