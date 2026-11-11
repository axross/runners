// the banners, the pool cards with their runners and diagnostics, and the footer: the
// text of every field is set as text.

import { controlButton, keyOf, nextId } from "./controls.js";
import { h } from "./dom.js";
import { icon } from "./icons.js";

/** the mark a runner's or a pool's state carries; the state is always written out too. */
function mark(name) {
  switch (name) {
    case "check":
      return icon("check", 20, "mark check");
    case "ring":
      return h(
        "span",
        { class: "mark", "aria-hidden": "true" },
        h("span", { class: "ring" }),
      );
    case "history":
      return icon("history", 20, "mark history");
    case "stop":
      return icon("square", 20, "mark stop");
    case "flag":
      return h(
        "span",
        { class: "flag", "aria-hidden": "true" },
        icon("alert", 20),
      );
    default:
      return icon("dash", 20, "mark unknown");
  }
}

function bannerMark(banner) {
  if (banner.tone === "error") {
    return mark("flag");
  }
  if (banner.tone === "warning") {
    return icon("warning", 20);
  }
  switch (banner.kind) {
    case "lost_contact":
      return icon("slashed", 20);
    case "building":
    case "stopping_build":
    case "starting":
    case "stopping":
    case "quitting":
      return mark("ring");
    default:
      return icon("info", 20);
  }
}

function bannerBody(banner, ctx) {
  const lines = banner.lines;
  const boxed = banner.tone === "error" && lines.length > 1;
  const details = boxed ? lines.slice(0, -1) : [];
  const plain = boxed ? lines.slice(-1) : lines;
  return h(
    "div",
    { class: "banner-body" },
    h("p", { class: "label", text: banner.heading }),
    details.length > 0
      ? h(
          "ul",
          { class: "details" },
          details.map((line) =>
            h(
              "li",
              { class: "description" },
              h("span", { class: "wrap", text: line }),
            ),
          ),
        )
      : null,
    plain.map((line) => h("p", { class: "description low wrap", text: line })),
    banner.action
      ? h(
          "div",
          { class: "banner-action" },
          controlButton(banner.action, {
            shape: "button",
            key: `banner:${keyOf(banner.action.action)}`,
            onPress: (button) => ctx.press(banner.action, button),
          }),
        )
      : null,
  );
}

/** the banners that lead the scrolling list, over both tabs. */
export function banners(list, ctx) {
  return list.map((banner) =>
    h(
      "section",
      { class: "banner", "data-kind": banner.kind },
      bannerMark(banner),
      bannerBody(banner, ctx),
    ),
  );
}

function runnerRow(card, row) {
  return h(
    "li",
    { class: "row" },
    h(
      "span",
      { class: "m", "aria-hidden": "true" },
      row.mark ? mark(row.mark) : null,
    ),
    h(
      "span",
      { class: "n" },
      h("span", { class: "vh", text: `${card.name}, ` }),
      row.name,
    ),
    h("span", { class: "s", text: row.state }),
    row.container
      ? h("span", { class: "c caption low wrap", text: row.container })
      : null,
    row.since ? h("span", { class: "t caption low", text: row.since }) : null,
    row.detail
      ? h("span", { class: "d caption low wrap", text: row.detail })
      : null,
  );
}

function diagnosticItem(item) {
  const where = item.from
    ? `${item.container} · from ${item.from}`
    : item.container;
  return h(
    "li",
    { class: "diagnostic" },
    h(
      "span",
      { class: "m", "aria-hidden": "true" },
      item.problem ? icon("warning", 20) : h("span", { class: "dot" }),
    ),
    h("span", { class: "x caption wrap", text: item.text }),
    h("span", { class: "c caption low wrap", text: where }),
  );
}

function diagnostics(section) {
  return h(
    "div",
    { class: "diagnostics" },
    h(
      "div",
      { class: "diagnostics-head" },
      h("h3", { text: section.heading }),
      h("span", { class: "caption low wrap", text: section.summary }),
    ),
    section.notes.map((note) =>
      h("p", { class: "notes caption low wrap", text: note }),
    ),
    section.items.length > 0
      ? h(
          "ul",
          { class: "diagnostic-items" },
          section.items.map(diagnosticItem),
        )
      : null,
  );
}

function card(poolCard, ctx) {
  const headingId = nextId("pool");
  const note = poolCard.note
    ? h(
        "div",
        { class: "card-note" },
        h("p", { class: "description wrap", text: poolCard.note }),
        poolCard.pill
          ? controlButton(poolCard.pill, {
              shape: "pill",
              key: `pill:${keyOf(poolCard.pill.action)}`,
              name: `${poolCard.pill.label} for ${poolCard.repository}`,
              onPress: () =>
                ctx.gotoToken(poolCard.repository, poolCard.pill.action),
            })
          : null,
      )
    : null;
  return h(
    "section",
    { class: "card", "aria-labelledby": headingId },
    h(
      "div",
      { class: "card-head" },
      h("h2", { id: headingId, class: "row-label wrap", text: poolCard.name }),
      h("span", { class: "caption low wrap", text: poolCard.repository }),
      h("span", { class: "grow" }),
      h(
        "span",
        { class: "caption card-state" },
        poolCard.mark ? mark(poolCard.mark) : null,
        poolCard.line,
      ),
    ),
    note,
    poolCard.runners.length > 0
      ? h(
          "ul",
          { class: "runners" },
          poolCard.runners.map((row) => runnerRow(poolCard, row)),
        )
      : null,
    poolCard.diagnostics ? diagnostics(poolCard.diagnostics) : null,
  );
}

/** the Pools panel: a card per pool, or what stands where they would be. */
export function poolsPanel(list, ctx) {
  const content =
    list.pools.length > 0
      ? h(
          "div",
          {
            class: `pools${list.stale ? " stale" : ""}`,
            role: "region",
            "aria-label": "Pools",
            tabindex: "0",
            "data-key": "pools",
          },
          list.pools.map((poolCard) => card(poolCard, ctx)),
        )
      : null;
  return [
    content,
    list.pools_note
      ? h("p", { class: "description low pools-note", text: list.pools_note })
      : null,
  ];
}

/** the footer: the image lines and warnings, the configuration path, and the note. */
export function footer(model) {
  return h(
    "footer",
    { class: "footer" },
    model.image_lines.map((line) =>
      h("p", { class: "caption low", text: line }),
    ),
    model.warnings.map((line) =>
      h(
        "p",
        { class: "caption warning" },
        icon("warning", 16),
        h("span", { text: line }),
      ),
    ),
    h("p", {
      class: "caption low wrap",
      text: `${model.configuration} · ${model.note}`,
    }),
  );
}
