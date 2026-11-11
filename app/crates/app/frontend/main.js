// the main window: it draws the view the app sends, and calls back only through the
// app's commands. It never navigates away, opens a window or keeps a token.

import { askConfirmation, cancelDialog, dialogOpen } from "./dialog.js";
import { keyOf } from "./controls.js";
import { say } from "./live.js";
import { renderWindow } from "./render.js";
import { closeTip, resetTips } from "./tooltip.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const app = document.getElementById("app");

const state = {
  view: null,
  /** the view as text, so that a view that has not changed is not drawn again. */
  drawn: "",
  /** the tab on show; chosen from the first list the window draws. */
  tab: null,
  /** what the last action that failed said, until the next action. */
  notice: null,
};

const RETRY_MS = 500;

// ---- reading the view

function listOf(view) {
  return view?.window.body.kind === "list" ? view.window.body : null;
}

function rowOf(view, repository) {
  return listOf(view)?.tokens.rows.find((row) => row.repository === repository);
}

/** which token forms the view shows open: the page's state is the view's. */
function openForms(view) {
  const rows = listOf(view)?.tokens.rows ?? [];
  return {
    editing: rows.find((row) => row.form)?.repository ?? null,
    removing: rows.find((row) => row.confirming_removal)?.repository ?? null,
  };
}

// ---- focus

function elementOfKey(key) {
  return [...app.querySelectorAll("[data-key]")].find(
    (element) => element.dataset.key === key,
  );
}

/** the key of the focused element, when focus is in the window's content. */
function focusedKey() {
  const element = document.activeElement;
  return element && app.contains(element)
    ? (element.dataset.key ?? null)
    : null;
}

function focusKey(key) {
  const element = elementOfKey(key);
  element?.focus();
  return element !== undefined;
}

function focusHeading() {
  focusKey("heading");
}

// ---- drawing

function draw(view, focusRequest = null) {
  const before = focusRequest ?? focusedKey();
  const scroller = elementOfKey("scroller");
  const scrolled = scroller ? scroller.scrollTop : 0;

  const list = listOf(view);
  if (list && state.tab === null) {
    const forms = openForms(view);
    state.tab =
      forms.editing || forms.removing ? "tokens" : list.header.opens_on;
  }
  state.view = view;
  state.drawn = JSON.stringify(view);

  resetTips();
  app.replaceChildren(...renderWindow(view, context));
  const next = elementOfKey("scroller");
  if (next) {
    next.scrollTop = scrolled;
  }
  if (before !== null && !focusKey(before)) {
    focusHeading();
  }
}

/** draws `view` when it differs from the one drawn, and says what the app announces. */
function apply(view, announce = [], { focus = null } = {}) {
  if (focus !== null || JSON.stringify(view) !== state.drawn) {
    draw(view, focus);
  }
  say(announce);
}

function redraw() {
  if (state.view) {
    draw(state.view);
  }
}

// ---- calling the app

function fail(error) {
  state.notice = String(error);
  say([state.notice]);
  redraw();
}

async function run(command, args = {}) {
  if (state.notice !== null) {
    state.notice = null;
    redraw();
  }
  try {
    await invoke(command, args);
  } catch (error) {
    fail(error);
  }
}

async function readView(forms) {
  const shown = await invoke("read_view", forms ? { forms } : {});
  return shown.view;
}

async function showForms(next, focus) {
  try {
    apply(await readView(next), [], { focus });
  } catch (error) {
    fail(error);
  }
}

// ---- tokens

function openForm(repository) {
  selectTab("tokens");
  showForms({ editing: repository, removing: null }, `field:${repository}`);
}

function openRemoval(repository) {
  selectTab("tokens");
  showForms(
    { editing: null, removing: repository },
    `cancel-remove:${repository}`,
  );
}

function cancelForm(repository) {
  const { removing } = openForms(state.view);
  showForms({ editing: null, removing }, `row:${repository}:primary`);
}

function cancelRemoval(repository) {
  const { editing } = openForms(state.view);
  showForms({ editing, removing: null }, `row:${repository}:remove`);
}

async function removeToken(repository) {
  await run("remove_token", { repository });
  try {
    apply(await readView(), [], { focus: `row:${repository}:primary` });
  } catch (error) {
    fail(error);
  }
}

/**
 * passes the field's token to the app and clears the field at once, whatever comes of
 * it. The page holds the token nowhere else, and shows no token back.
 */
async function saveToken(repository, field) {
  if (state.notice !== null) {
    state.notice = null;
  }
  const sent = invoke("set_token", { repository, token: field.value });
  field.value = "";
  field.dispatchEvent(new Event("input"));
  try {
    await sent;
    const view = await readView();
    const failed = rowOf(view, repository)?.form;
    apply(view, [], {
      focus: failed ? `field:${repository}` : `row:${repository}:primary`,
    });
  } catch (error) {
    fail(error);
  }
}

// ---- controls

/** asks `control`'s question in the window, then runs `command`. */
function ask(question, { command, iconName, returnTo }) {
  if (!question) {
    run(command);
    return;
  }
  askConfirmation(question, {
    iconName,
    confirmed: () => run(command),
    returnFocus: () => returnTo !== null && focusKey(returnTo),
  });
}

/** runs `control`'s action; `button` is the element pressed, whose key focus returns to. */
function press(control, button) {
  const action = control.action;
  const key = button?.dataset.key ?? keyOf(action);
  if (typeof action !== "string") {
    const [[name, repository]] = Object.entries(action);
    if (name === "remove_token") {
      openRemoval(repository);
    } else {
      openForm(repository);
    }
    return;
  }
  switch (action) {
    case "start_all":
    case "try_again":
      run(action);
      break;
    case "build_image":
      run("build");
      break;
    case "stop_all":
    case "stop_build":
      ask(control.confirmation, {
        command: action,
        iconName: "square",
        returnTo: key,
      });
      break;
    default:
      break;
  }
}

function selectTab(name) {
  state.tab = name;
  for (const tab of app.querySelectorAll("[role=tab]")) {
    const selected = tab.dataset.tab === name;
    tab.setAttribute("aria-selected", String(selected));
    tab.setAttribute("tabindex", selected ? "0" : "-1");
  }
  for (const panel of app.querySelectorAll("[role=tabpanel]")) {
    panel.hidden = panel.id !== `panel-${name}`;
  }
}

/** a card's pill selects the Tokens tab with focus on the repository's row. */
function gotoToken(repository) {
  selectTab("tokens");
  focusKey(`row:${repository}:primary`);
}

const context = {
  get tab() {
    return state.tab;
  },
  get notice() {
    return state.notice;
  },
  press,
  selectTab,
  gotoToken,
  saveToken,
  cancelForm,
  cancelRemoval,
  removeToken,
};

// ---- keys

/** the token form or question around the focused element, with its repository. */
function openAround(element) {
  const form = element?.closest?.(".token-form, .token-confirm");
  const repository = form?.closest("[data-repository]")?.dataset.repository;
  return form && repository
    ? { repository, removal: form.classList.contains("token-confirm") }
    : null;
}

function escape(event) {
  event.preventDefault();
  if (closeTip()) {
    return;
  }
  if (dialogOpen()) {
    cancelDialog();
    return;
  }
  const around = openAround(document.activeElement);
  if (around) {
    (around.removal ? cancelRemoval : cancelForm)(around.repository);
    return;
  }
  invoke("hide_window");
}

/** arrow keys, Home and End move between the tabs, which then show their panel. */
function moveBetweenTabs(event) {
  const tabs = [...app.querySelectorAll("[role=tab]")];
  const at = tabs.indexOf(event.target);
  const target = {
    ArrowRight: tabs[(at + 1) % tabs.length],
    ArrowLeft: tabs[(at - 1 + tabs.length) % tabs.length],
    Home: tabs[0],
    End: tabs[tabs.length - 1],
  }[event.key];
  if (at >= 0 && target) {
    event.preventDefault();
    selectTab(target.dataset.tab);
    target.focus();
  }
}

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    escape(event);
  } else {
    moveBetweenTabs(event);
  }
});

document.addEventListener("visibilitychange", () => {
  if (!document.hidden && !dialogOpen()) {
    focusHeading();
  }
});

// ---- starting

listen("marshal:view", (event) =>
  apply(event.payload.view, event.payload.announce),
);

listen("marshal:confirm", (event) => {
  const { action, question } = event.payload;
  const opener = focusedKey();
  ask(question, { command: action, iconName: "square", returnTo: opener });
});

async function load() {
  for (;;) {
    try {
      apply(await readView(), []);
      focusHeading();
      return;
    } catch {
      // the app has no view to give yet: ask again in a moment
      await new Promise((resolve) => setTimeout(resolve, RETRY_MS));
    }
  }
}

load();
