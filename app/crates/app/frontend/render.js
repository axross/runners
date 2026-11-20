// the whole window from one view: the header with its tabs, the scrolling list, the
// footer, or the one message that stands in place of the list while Marshal does not
// control the runners.

import { controlButton, keyOf } from "./controls.js";
import { h } from "./dom.js";
import { icon } from "./icons.js";
import { DRAG_REGION } from "./platform.js";
import { banners, footer, poolsPanel } from "./pools.js";
import { tokensPanel } from "./tokens.js";

const TABS = [
  ["pools", "Pools"],
  ["tokens", "Tokens"],
];

function tabs(header, ctx) {
  const buttons = TABS.map(([name, label]) => {
    const selected = ctx.tab === name;
    return h(
      "button",
      {
        type: "button",
        role: "tab",
        id: `tab-${name}`,
        class: "tab",
        "data-key": `tab:${name}`,
        "data-tab": name,
        "aria-controls": `panel-${name}`,
        "aria-selected": String(selected),
        tabindex: selected ? "0" : "-1",
        on: { click: () => ctx.selectTab(name) },
      },
      h("span", { text: label }),
      name === "tokens" && header.tokens_chip
        ? [
            h(
              "span",
              { class: "chip", "aria-hidden": "true" },
              icon("alert", 16),
            ),
            h("span", { class: "vh", text: `, ${header.tokens_chip}` }),
          ]
        : null,
    );
  });
  return h(
    "div",
    { class: "tabs-row" },
    h("div", { class: "tabs", role: "tablist", "aria-label": "View" }, buttons),
  );
}

function header(list, ctx) {
  const model = list.header;
  return h(
    "header",
    {
      class: "header",
      "data-buttons": String(model.controls.length),
      "data-tauri-drag-region": DRAG_REGION,
    },
    h(
      "div",
      { class: "summary" },
      h("h1", {
        class: "heading",
        tabindex: "-1",
        "data-key": "heading",
        text: model.heading,
      }),
      h("p", { class: "description low", text: model.line }),
    ),
    h(
      "div",
      { class: "actions" },
      model.controls.map((spec) =>
        controlButton(spec, {
          shape: "button",
          key: keyOf(spec.action),
          onPress: (button) => ctx.press(spec, button),
        }),
      ),
    ),
    ctx.notice
      ? h("p", { class: "description notice wrap", text: ctx.notice })
      : null,
    tabs(model, ctx),
  );
}

function panel(name, ctx, ...children) {
  return h(
    "div",
    {
      class: "panel",
      role: "tabpanel",
      id: `panel-${name}`,
      "aria-labelledby": `tab-${name}`,
      hidden: ctx.tab !== name,
    },
    children,
  );
}

function listWindow(list, ctx) {
  return [
    header(list, ctx),
    h(
      "main",
      {
        class: "scroller",
        "aria-label": "Pools and tokens",
        "data-key": "scroller",
      },
      banners(list.banners, ctx),
      panel("pools", ctx, poolsPanel(list, ctx)),
      panel("tokens", ctx, tokensPanel(list.tokens, list.stale, ctx)),
    ),
  ];
}

function fullWindow(full, ctx) {
  return h(
    "main",
    { class: "full", "data-tauri-drag-region": DRAG_REGION },
    h(
      "span",
      { class: "full-mark", "aria-hidden": "true" },
      icon(full.action ? "slashed" : "info", 24),
    ),
    h(
      "div",
      { class: "full-text" },
      h("h1", {
        class: "heading",
        tabindex: "-1",
        "data-key": "heading",
        text: full.heading,
      }),
      full.lines.map((line) =>
        h("p", { class: "description low wrap", text: line }),
      ),
    ),
    full.action
      ? controlButton(full.action, {
          shape: "button",
          key: keyOf(full.action.action),
          onPress: (button) => ctx.press(full.action, button),
        })
      : null,
  );
}

/** the elements of the window for `view`, in the order they appear. */
export function renderWindow(view, ctx) {
  const body = view.window.body;
  const content =
    body.kind === "full" ? [fullWindow(body, ctx)] : listWindow(body, ctx);
  return [...content, footer(view.window.footer)];
}
