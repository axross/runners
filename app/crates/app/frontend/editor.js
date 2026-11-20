// the in-card editor and its in-list review. Values and problems are text; the
// agent's effects, never a second set of apply rules, supply the confirmation.

import { controlButton, describe, nextId } from "./controls.js";
import { h } from "./dom.js";
import { attachTip } from "./tooltip.js";

/** an unavailable editor action stays focusable and explains why it cannot run. */
function button(
  key,
  label,
  press,
  reason = null,
  look = "neutral",
  name = label,
) {
  return controlButton(
    { label, available: !reason, reason, look },
    {
      shape: "pill",
      key,
      name,
      onPress: press,
    },
  );
}

/** a problem names its TOML key and optional source line without quoting a value. */
function problemText(problem) {
  return [
    problem.path,
    problem.line ? `line ${problem.line}` : null,
    problem.message,
  ]
    .filter(Boolean)
    .join(": ");
}

function problems(model) {
  return (
    model.error?.problems ??
    (model.problemsInResult ? [] : (model.document?.problems ?? []))
  );
}

/** a labelled field with its problem as an accessible description, even when locked. */
function field(model, row, spec, prefixes) {
  const at = (prefix) => (prefix ? `${prefix}.${spec.key}` : spec.key);
  const path = at(prefixes.shown);
  const id = nextId("config-field");
  const description = `${id}-description`;
  const errors =
    prefixes.matched === null
      ? []
      : problems(model).filter((p) => p.path === at(prefixes.matched));
  const unavailable = model.locked
    ? "Finish or cancel the check and review first."
    : null;
  const props = {
    id,
    class: "field config-input",
    "data-key": `config-field:${row.id}:${spec.key}`,
    "aria-describedby": description,
    "aria-invalid": errors.length ? "true" : null,
    "aria-disabled": unavailable ? "true" : null,
    on: {
      input: (event) => {
        if (model.locked) {
          event.target.value = row.fields[spec.key];
          return;
        }
        model.set(row, spec.key, event.target.value);
      },
    },
  };
  let input;
  const options =
    spec.kind === "boolean"
      ? [
          ["true", "On"],
          ["false", "Off"],
        ]
      : spec.kind === "os"
        ? [
            ["linux", "Linux"],
            ["macos", "macOS"],
          ]
        : spec.kind === "linux"
          ? [["linux", "Linux"]]
          : null;
  if (options) {
    const current = row.fields[spec.key];
    if (current && !options.some(([value]) => value === current))
      options.push([current, `${current} — not supported`]);
    input = h(
      "select",
      props,
      [["", spec.absent], ...options].map(([value, label]) =>
        h("option", { value, text: label }),
      ),
    );
  } else if (spec.kind === "texts") {
    input = h("textarea", { ...props, rows: "2", readonly: model.locked });
  } else {
    input = h("input", {
      ...props,
      type: "text",
      readonly: model.locked,
      autocomplete: "off",
      spellcheck: "false",
      inputmode: ["number", "integer"].includes(spec.kind) ? "decimal" : null,
    });
  }
  input.value = row.fields[spec.key] ?? "";
  const control =
    spec.key === "runners"
      ? h(
          "div",
          { class: "config-stepper" },
          input,
          ...[-1, 1].map((step) =>
            button(
              `${step < 0 ? "fewer" : "more"}-runners:${row.id}`,
              step < 0 ? "−" : "+",
              () => {
                const current = Number(row.fields[spec.key]);
                const base = Number.isSafeInteger(current) ? current : 0;
                model.set(row, spec.key, String(Math.max(0, base + step)));
              },
              unavailable ??
                (step < 0 && Number(row.fields[spec.key]) <= 0
                  ? "A pool cannot have fewer than 0 runners."
                  : null),
              "neutral",
              `${step < 0 ? "Fewer" : "More"} runners for ${row.fields.name || "Unnamed pool"}`,
            ),
          ),
        )
      : input;
  const wrap = h("div", { class: "tipwrap" }, control);
  if (unavailable) attachTip(wrap, () => unavailable);
  return h(
    "div",
    { class: `config-field${errors.length ? " invalid" : ""}` },
    h("label", { for: id, class: "caption label", text: spec.label }),
    wrap,
    errors.length &&
      row.original[spec.key] === "" &&
      row.fields[spec.key] !== null
      ? button(
          `unset-config:${row.id}:${spec.key}`,
          "Remove invalid value",
          () => model.set(row, spec.key, null),
          unavailable,
          "neutral",
          `Remove invalid value for ${spec.label}`,
        )
      : null,
    h("p", {
      id: description,
      class: `caption wrap ${errors.length ? "config-error" : "low"}`,
      text: [
        ...errors.map(problemText),
        unavailable,
        !row.fields[spec.key] ? spec.absent : null,
        path,
      ]
        .filter(Boolean)
        .join(" · "),
    }),
  );
}

/** replaces only the edited card's runner rows; no field changes the running pool. */
function form(model, pool) {
  const prefixes = model.problemPrefixes(pool);
  return h(
    "div",
    { class: "config-form" },
    h("p", {
      class: "caption low wrap",
      text: "The running pool keeps its current settings until Apply.",
    }),
    h(
      "div",
      { class: "config-grid" },
      model.access.pool.map((spec) => field(model, pool, spec, prefixes)),
    ),
    model.access.volume.length
      ? h(
          "div",
          { class: "config-volumes" },
          h("h3", { class: "label caption", text: "Volumes" }),
          pool.volumes
            .filter((v) => !v.removed)
            .map((volume) =>
              h(
                "div",
                { class: "config-volume" },
                h(
                  "div",
                  { class: "config-grid" },
                  model.access.volume.map((spec) =>
                    field(
                      model,
                      volume,
                      spec,
                      model.volumePrefixes(pool, volume),
                    ),
                  ),
                ),
                button(
                  `remove-volume:${volume.id}`,
                  "Remove Volume",
                  () => model.removeVolume(pool, volume),
                  model.locked ? "Finish or cancel the review first." : null,
                  "destructive",
                ),
              ),
            ),
          button(
            `add-volume:${pool.id}`,
            "Add Volume",
            () => model.addVolume(pool),
            model.locked ? "Finish or cancel the review first." : null,
          ),
        )
      : null,
    h(
      "div",
      { class: "config-actions" },
      button(
        `remove-pool:${pool.id}`,
        "Remove Pool",
        () => model.removePool(pool),
        model.locked ? "Finish or cancel the review first." : null,
        "destructive",
      ),
      button(`done-pool:${pool.id}`, "Done", () => {
        model.editing = null;
        model.changed(`edit-pool:${pool.id}`);
      }),
    ),
  );
}

/** draft cards retain the live pool's rows outside Edit and mark unapplied changes. */
export function editorPools(model, list, ctx, liveCard) {
  const reason = model.locked
    ? "Finish or cancel the review first."
    : model.fileReason;
  if (!model.document || model.fileReason)
    return [
      ...list.pools.map((pool) => liveCard(pool, ctx)),
      !model.document
        ? h("p", { class: "description low wrap", text: model.fileReason })
        : null,
      button(
        "read-config",
        "Read Configuration",
        () => model.read(),
        model.busy ? "Reading the configuration." : model.access.read_reason,
      ),
    ];
  const cards = model.pools.map((pool) => {
    const name = pool.fields.name || "Unnamed pool";
    if (pool.removed)
      return h(
        "section",
        { class: "card config-removed" },
        h("h2", { class: "row-label wrap", text: name }),
        h("p", {
          class: "description wrap",
          text: "This pool will be removed when you apply.",
        }),
        button(
          `keep-pool:${pool.id}`,
          "Keep",
          () => model.keepPool(pool),
          reason,
        ),
      );
    const live =
      pool.source === null
        ? null
        : list.pools.find(
            (card) => card.name === model.document.pools[pool.source].name,
          );
    const card =
      model.editing !== pool.id && live
        ? liveCard(live, ctx)
        : h(
            "section",
            { class: "card config-card" },
            h(
              "div",
              { class: "card-head" },
              h("h2", {
                class: "row-label wrap",
                tabindex: "-1",
                "data-key": `edit-heading:${pool.id}`,
                text: name,
              }),
              h("span", {
                class: "caption low wrap",
                text: pool.fields.repository,
              }),
              h("span", { class: "grow" }),
              h("span", {
                class: "caption low",
                text: pool.source === null ? "New pool" : "Editing",
              }),
            ),
            model.editing === pool.id ? form(model, pool) : null,
          );
    if (model.editing !== pool.id) {
      const head = card.querySelector(".card-head");
      head
        .querySelector("h2")
        .setAttribute("data-key", `edit-heading:${pool.id}`);
      const { matched } = model.problemPrefixes(pool);
      const hasProblems =
        matched !== null &&
        problems(model).some(
          (p) => p.path === matched || p.path?.startsWith(`${matched}.`),
        );
      const changed = model.poolChanged(pool);
      if (hasProblems || changed)
        head.append(
          h("span", {
            class: "caption config-chip",
            text: hasProblems ? "Problems" : "Changes",
          }),
        );
      head.append(
        button(
          `edit-pool:${pool.id}`,
          "Edit",
          () => {
            model.applyResult = null;
            model.editing = pool.id;
            model.changed(`edit-heading:${pool.id}`);
          },
          reason,
        ),
      );
    }
    return card;
  });
  const fileNames = new Set(model.document.pools.map((pool) => pool.name));
  const remaining = list.pools.filter((pool) => !fileNames.has(pool.name));
  return [
    h(
      "div",
      { class: "pools config-pools" },
      cards,
      remaining.map((pool) => liveCard(pool, ctx)),
    ),
    button("add-pool", "Add Pool", () => model.addPool(), reason),
  ];
}

/** terminal token results retain the retry's focus key but cannot remove another token. */
function tokenRemoval(model, repository, result) {
  const label = {
    failed: "Try Again",
    removed: "Removed",
    cancelled: "Cancelled",
    unanswered: "Not Answered",
  }[result.state];
  const message =
    result.state === "cancelled"
      ? `The earlier token-removal choice for ${repository} was cancelled when the repository returned to the configuration.`
      : result.state === "removed"
        ? `The stored token for ${repository} was removed. The configuration stays saved.`
        : result.state === "unanswered"
          ? `The agent has not answered about removing the token for ${repository}, so it may or may not still be stored. The configuration stays saved.`
          : `The token for ${repository} was not removed: ${result.cause}. The configuration stays saved.`;
  const reason = model.busy
    ? "Waiting for the agent's answer."
    : model.review
      ? "Finish or cancel the check and review first."
      : (model.reason ?? model.fileReason);
  return h(
    "div",
    { class: "config-token-failure" },
    h("p", { class: "description wrap", text: message }),
    button(
      `retry-token:${repository}`,
      label,
      () => model.retryToken(repository),
      result.state === "failed" ? reason : message,
    ),
  );
}

/** request results and problems that may have no field, including retryable store failures. */
export function editorNotice(model) {
  if (!model.access) return null;
  return h(
    "section",
    { class: "config-notice", "aria-label": "Configuration result" },
    model.message && model.message !== model.fileReason
      ? h("p", { class: "description wrap", text: model.message })
      : null,
    model.reason
      ? h("p", { class: "caption low wrap", text: model.reason })
      : null,
    model.fileReason && model.document
      ? h("p", { class: "caption config-error wrap", text: model.fileReason })
      : null,
    problems(model).map((problem) =>
      h("p", {
        class: "caption config-error wrap",
        text: problemText(problem),
      }),
    ),
    problems(model).length
      ? h("p", {
          class: "caption low wrap",
          text: "A problem with no matching field must be repaired in a text editor, then reloaded.",
        })
      : null,
    model.error?.code === "config_changed"
      ? h(
          "div",
          {},
          h("p", {
            class: "description wrap",
            text: "Read Again drops your unapplied changes and reads config.toml. Nothing is applied.",
          }),
          button(
            "read-again-config",
            "Read Again",
            () => model.read(),
            model.access.read_reason,
          ),
        )
      : null,
    [...model.tokenRemovals].map(([repository, result]) =>
      tokenRemoval(model, repository, result),
    ),
  );
}

const EFFECT = {
  unchanged: "Unchanged. Its runners and jobs carry on.",
  added: "Added. Its runners start under Start All's rules.",
  removed: "Removed.",
  runners_added: "Gains runners. The other slots carry on.",
  runners_removed:
    "Loses its highest-numbered slots. The other slots carry on.",
  restarted: "Restarts with the new settings.",
};

/** what one pool's effect does, naming the jobs that fail only when the effect stops runners. */
function effectText(effect) {
  const text = EFFECT[effect.change];
  if (!text)
    return "Unknown effect. Cancel and update Marshal before applying.";
  if (!effect.stops_runners) return text;
  return effect.change === "runners_removed"
    ? `${text} The jobs on the lost slots fail.`
    : `${text} Its running jobs fail.`;
}

/** the agent's effects supply the warning; unknown effects cannot be confirmed. */
export function editorReview(model) {
  if (!model.review)
    return model.applyResult
      ? h(
          "section",
          { class: "card config-review", "aria-label": model.applyResult },
          h("h2", { class: "row-label", text: model.applyResult }),
          h(
            "div",
            { class: "config-actions" },
            button(
              "confirm-config",
              "Done",
              () => model.cancel(),
              model.busy ? "Waiting for the agent's answer." : null,
            ),
          ),
        )
      : null;
  const { change, remove } = model.review;
  const stops = change.effects.some((effect) => effect.stops_runners);
  return h(
    "section",
    {
      class: "card config-review",
      "aria-label": "Review configuration changes",
    },
    h("h2", { class: "row-label", text: "Apply these changes?" }),
    h(
      "ul",
      { class: "config-effects" },
      change.effects.map((effect) =>
        h(
          "li",
          { class: "description wrap" },
          h("strong", { text: `${effect.pool}: ` }),
          `${effectText(effect)} (${effect.runners_before} → ${effect.runners_after})`,
        ),
      ),
    ),
    h("p", {
      class: "description wrap",
      text: !change.applies
        ? "Saved settings take effect at the next Start All."
        : stops
          ? "Runners stop at once, and any job running on them fails. Unchanged pools keep running."
          : "No running jobs are stopped by this change.",
    }),
    change.repositories_released.map((repository) => {
      const label = `Also remove the stored token for ${repository}. No pool will use it. Keeping it is the default.`;
      const reason = model.busy ? "Waiting for the agent's answer." : null;
      const input = h("input", {
        type: "checkbox",
        "data-key": `release-token:${repository}`,
        "aria-label": label,
        "aria-disabled": reason ? "true" : null,
        on: {
          change: () => {
            if (model.busy) {
              input.checked = remove.has(repository);
              return;
            }
            input.checked ? remove.add(repository) : remove.delete(repository);
          },
        },
      });
      input.checked = remove.has(repository);
      const wrap = h("span", { class: "tipwrap" }, input);
      if (reason) describe(wrap, input, reason);
      return h(
        "label",
        { class: "config-checkbox description" },
        wrap,
        h("span", {
          class: "wrap",
          text: label,
        }),
      );
    }),
    h(
      "div",
      { class: "config-actions" },
      button(
        "cancel-config",
        "Cancel",
        () => model.cancel(),
        model.busy ? "Waiting for the agent's answer." : null,
      ),
      button(
        "confirm-config",
        stops ? "Apply and Stop Runners" : "Apply",
        () => model.apply(),
        model.busy
          ? "Waiting for the agent's answer."
          : (model.reason ??
              (change.effects.some((e) => !EFFECT[e.change])
                ? "The agent listed an unknown effect."
                : null)),
        stops ? "destructive" : "solid",
      ),
    ),
  );
}

/** the draft's actions stay above the footer, separate from the apply confirmation. */
export function editorBar(model) {
  if (!model.access || model.review) return null;
  if (model.readAsk)
    return h(
      "div",
      { class: "config-bar" },
      h("span", {
        class: "description grow wrap",
        text: "Read Again drops your unapplied changes. Nothing is applied.",
      }),
      button("keep-editing", "Keep Editing", () => model.cancel()),
      button(
        "read-confirm",
        "Read Again",
        () => model.read(true),
        model.access.read_reason,
        "destructive",
      ),
    );
  if (model.reloadAsk)
    return h(
      "div",
      { class: "config-bar" },
      h("span", {
        class: "description grow wrap",
        text: "Reload drops your unapplied changes and reads config.toml again.",
      }),
      button("keep-editing", "Keep Editing", () => model.cancel()),
      button(
        "reload-now",
        "Reload",
        () => model.check(true),
        null,
        "destructive",
      ),
    );
  if (model.applyResult || (!model.dirty && !model.busy)) return null;
  const reason = model.busy
    ? "Waiting for the agent's answer."
    : (model.reason ??
      model.fileReason ??
      (model.error?.code === "config_changed"
        ? "Read Again before applying."
        : !model.dirty
          ? "There are no changes to apply."
          : null));
  return h(
    "div",
    { class: "config-bar" },
    h("span", {
      class: "description grow wrap",
      text: model.busy ? "Checking or applying…" : "Unapplied changes",
    }),
    button(
      "discard-config",
      "Discard",
      () => model.discard(),
      model.locked ? "Waiting for the agent's answer." : null,
    ),
    button("check-config", "Apply", () => model.check(), reason, "solid"),
  );
}

/** host-wide fields and explicit Reload; neither polls nor opens the file. */
export function editorFooter(model, configuration) {
  if (!model.access) return null;
  return h(
    "div",
    { class: "config-footer" },
    h(
      "div",
      { class: "config-actions" },
      h("p", { class: "caption low wrap grow", text: configuration }),
      button(
        "reload-config",
        "Reload config.toml",
        () => model.check(true),
        model.locked ? "Finish or cancel the review first." : model.reason,
      ),
    ),
    h(
      "div",
      { class: "config-actions" },
      model.host
        ? h("p", {
            class: "caption low wrap grow",
            text: model.access.settings
              .map(
                (spec) =>
                  `${spec.label}: ${model.host.fields[spec.key] || spec.absent}`,
              )
              .join(" · "),
          })
        : null,
      button(
        "host-settings",
        model.hostOpen ? "Done" : "Edit Host Settings",
        () => {
          model.applyResult = null;
          if (!model.document) {
            model.read().then(() => {
              model.hostOpen = !model.fileReason;
              model.changed();
            });
          } else {
            model.hostOpen = !model.hostOpen;
            model.changed();
          }
        },
        model.locked
          ? "Finish or cancel the review first."
          : ((model.document ? model.fileReason : null) ??
              model.access.read_reason),
      ),
    ),
    model.hostOpen && model.host && !model.fileReason
      ? h(
          "section",
          { "aria-label": "Host settings", class: "config-host" },
          h("h2", { class: "row-label", text: "Host settings" }),
          h(
            "div",
            { class: "config-grid" },
            model.access.settings.map((spec) =>
              field(model, model.host, spec, { shown: "", matched: "" }),
            ),
          ),
        )
      : null,
  );
}
