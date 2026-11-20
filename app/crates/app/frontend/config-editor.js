// the window's unapplied configuration, kept in memory across hides. Checks and
// writes go only through the agent; a check's preview never replaces its source revision.

const STATE_CHANGED =
  "Marshal's state changed. Check and review again before applying. Your changes are kept.";

function displayed(value) {
  return value === undefined
    ? ""
    : Array.isArray(value)
      ? value.join("\n")
      : String(value);
}

/** a draft row retains its original strings, so an untouched default stays absent. */
function table(values, specs, source, id) {
  const fields = Object.fromEntries(
    specs.map(({ key }) => [key, displayed(values[key])]),
  );
  return { source, id, fields, original: { ...fields }, removed: false };
}

/** parses a changed field, leaving an empty one unset.
 * @throws a field problem when a numeric value cannot be sent without loss.
 */
function scalar(text, spec, path) {
  if (text === "" || text === null) return undefined;
  switch (spec.kind) {
    case "integer":
    case "number": {
      const number = Number(text);
      if (
        !text.trim() ||
        !Number.isFinite(number) ||
        (spec.kind === "integer" && !Number.isSafeInteger(number))
      ) {
        throw {
          code: "config_invalid",
          message: "Fix the marked field.",
          problems: [
            {
              path,
              message:
                spec.kind === "integer"
                  ? "must be a whole number"
                  : "must be a number",
            },
          ],
        };
      }
      return number;
    }
    case "boolean":
      return text === "true";
    case "texts":
      return text.split("\n");
    default:
      return text;
  }
}

/** the set fields of a new entry.
 * @throws the problem from an invalid numeric field.
 */
function values(row, specs, prefix) {
  return Object.fromEntries(
    specs.flatMap((spec) => {
      const value = scalar(row.fields[spec.key], spec, `${prefix}.${spec.key}`);
      return value === undefined ? [] : [[spec.key, value]];
    }),
  );
}

/** only changed fields become operations; the source row's indices still apply.
 * @throws the problem from an invalid numeric field.
 */
function changedFields(row, specs, prefix) {
  return specs.flatMap((spec) => {
    const text = row.fields[spec.key];
    if (text === row.original[spec.key]) return [];
    const path = prefix ? `${prefix}.${spec.key}` : spec.key;
    const value = scalar(text, spec, path);
    return [
      value === undefined ? { op: "remove", path } : { op: "set", path, value },
    ];
  });
}

/** updates source entries before removals shift indices, then appends new entries.
 * @throws the problem from an invalid numeric field.
 */
function volumeEdits(pool, specs, prefix) {
  const edits = pool.volumes
    .filter((v) => v.source !== null && !v.removed)
    .flatMap((v) => changedFields(v, specs, `${prefix}.volumes[${v.source}]`));
  for (const volume of pool.volumes
    .filter((v) => v.source !== null && v.removed)
    .reverse()) {
    edits.push({
      op: "remove_entry",
      path: `${prefix}.volumes[${volume.source}]`,
    });
  }
  let index = pool.volumes.filter(
    (v) => v.source !== null && !v.removed,
  ).length;
  for (const volume of pool.volumes.filter(
    (v) => v.source === null && !v.removed,
  )) {
    edits.push({
      op: "add_entry",
      array: `${prefix}.volumes`,
      values: values(volume, specs, `${prefix}.volumes[${index}]`),
    });
    index += 1;
  }
  return edits;
}

/** an in-memory draft; the agent alone checks, writes and applies its operations. */
export class ConfigEditor {
  /** `changed` redraws with an optional focus key; `announce` uses the polite region. */
  constructor(invoke, changed, announce) {
    this.invoke = invoke;
    this.changed = changed;
    this.announce = announce;
    this.access = null;
    this.document = null;
    this.pools = [];
    this.host = null;
    this.editing = null;
    this.hostOpen = false;
    this.busy = false;
    this.review = null;
    this.applyResult = null;
    this.error = null;
    this.message = "";
    this.tokenRemovals = new Map();
    this.nextId = 0;
    this.opener = null;
    this.reloadAsk = false;
    this.readAsk = false;
    this.problemsInResult = false;
  }

  /** view updates may invalidate consent, but must not replace the user's draft. */
  update(access) {
    this.access = access ?? null;
    if (
      this.review &&
      (this.reason || this.review.change.applies === this.access.stopped)
    ) {
      this.review = null;
      this.message = STATE_CHANGED;
      this.announce([this.message]);
    }
  }

  get reason() {
    return (
      this.access?.reason ??
      (this.access ? null : "The agent does not offer configuration editing.")
    );
  }
  get locked() {
    return this.busy || this.review !== null;
  }
  /** a read may succeed while the file's syntax or form cannot be edited here. */
  get fileReason() {
    const doc = this.document;
    if (!doc) return "Read config.toml first.";
    if (doc.too_large)
      return "The editable values do not fit one message. Edit config.toml in a text editor.";
    if (!["valid", "invalid"].includes(doc.status))
      return "config.toml cannot be edited here. Fix or restore it in a text editor, then reload.";
    if (this.error?.code === "config_file_form")
      return "This file's form cannot be changed here. Repair it in a text editor, then reload.";
    return null;
  }

  /** whether discarding the draft would lose any input or entry operation. */
  get dirty() {
    if (!this.host) return false;
    const changed = (row) =>
      row.removed ||
      row.source === null ||
      Object.keys(row.fields).some(
        (key) => row.fields[key] !== row.original[key],
      );
    return (
      changed(this.host) ||
      this.pools.some((pool) => changed(pool) || pool.volumes.some(changed))
    );
  }

  notify(message, focus = null) {
    this.message = message;
    this.announce([message]);
    this.changed(focus);
  }

  /** replaces the draft with an agent read, without writing or applying anything. */
  loaded(doc) {
    const referenced = new Set(
      doc.pools
        .filter((pool) => typeof pool.repository === "string")
        .map((pool) => pool.repository.toLowerCase()),
    );
    for (const [repository, result] of this.tokenRemovals) {
      if (result.state === "failed" && referenced.has(repository.toLowerCase()))
        this.tokenRemovals.set(repository, { state: "cancelled" });
    }
    this.document = doc;
    this.error = null;
    this.problemsInResult = false;
    this.host = table(
      Object.fromEntries(
        this.access.settings.map(({ key }) => [
          key,
          key.split(".").reduce((value, part) => value?.[part], doc.settings),
        ]),
      ),
      this.access.settings,
      0,
      "host",
    );
    this.pools = doc.pools.map((pool, index) => {
      const row = table(pool, this.access.pool, index, this.nextId++);
      row.volumes = (pool.volumes ?? []).map((volume, index) =>
        table(volume, this.access.volume, index, this.nextId++),
      );
      return row;
    });
    this.editing = null;
    this.hostOpen = false;
  }

  /** asks before replacing a changed draft; a failed read retains the old draft. */
  async read(discard = false) {
    if (this.busy || this.access?.read_reason || !this.access) return;
    if (this.dirty && !discard) {
      this.readAsk = true;
      this.changed("keep-editing");
      return;
    }
    this.readAsk = false;
    this.applyResult = null;
    this.busy = true;
    this.changed();
    try {
      const answer = await this.invoke("configuration", {
        request: { kind: "read" },
      });
      this.loaded(answer.value);
      this.notify(this.fileReason ?? "config.toml read. Nothing was applied.");
    } catch (error) {
      this.refused(error);
    } finally {
      this.busy = false;
      this.changed();
    }
  }

  /** operations address source indices until removals, then append at result indices.
   * @throws the problem from an invalid numeric field.
   */
  edits() {
    const edits = changedFields(this.host, this.access.settings, "");
    for (const pool of this.pools.filter(
      (p) => p.source !== null && !p.removed,
    )) {
      const prefix = `pool[${pool.source}]`;
      edits.push(
        ...changedFields(pool, this.access.pool, prefix),
        ...volumeEdits(pool, this.access.volume, prefix),
      );
    }
    for (const pool of this.pools
      .filter((p) => p.source !== null && p.removed)
      .reverse()) {
      edits.push({ op: "remove_entry", path: `pool[${pool.source}]` });
    }
    let index = this.pools.filter(
      (p) => p.source !== null && !p.removed,
    ).length;
    for (const pool of this.pools.filter(
      (p) => p.source === null && !p.removed,
    )) {
      edits.push({
        op: "add_entry",
        array: "pool",
        values: values(pool, this.access.pool, `pool[${index}]`),
      });
      edits.push(...volumeEdits(pool, this.access.volume, `pool[${index}]`));
      index += 1;
    }
    return edits;
  }

  set(row, key, value) {
    if (this.locked) return;
    this.applyResult = null;
    row.fields[key] = value;
    this.changed();
  }

  /** a new pool is only a draft and receives focus before any agent request. */
  addPool() {
    if (this.locked || this.fileReason) return;
    this.applyResult = null;
    const pool = table(
      { name: `pool-${this.nextId + 1}`, runners: 1 },
      this.access.pool,
      null,
      this.nextId++,
    );
    pool.volumes = [];
    this.pools.push(pool);
    this.editing = pool.id;
    this.notify(
      "Pool added to the draft. Nothing runs until Apply.",
      `edit-heading:${pool.id}`,
    );
  }

  /** keeps source entries reversible until Apply; new draft entries need no tombstone. */
  removePool(pool) {
    if (this.locked) return;
    this.applyResult = null;
    if (pool.source === null) this.pools = this.pools.filter((p) => p !== pool);
    else pool.removed = true;
    this.editing = null;
    const next = this.pools.find((p) => !p.removed && p.id > pool.id);
    this.notify(
      "Pool marked for removal. Nothing changes until Apply.",
      next ? `edit-heading:${next.id}` : "add-pool",
    );
  }

  addVolume(pool) {
    if (this.locked) return;
    this.applyResult = null;
    pool.volumes.push(table({}, this.access.volume, null, this.nextId++));
    this.changed();
  }

  discard() {
    if (this.locked || !this.document) return;
    this.applyResult = null;
    this.loaded(this.document);
    this.notify("Unapplied changes discarded.", "heading");
  }

  /** drops the review, never the draft, when a request is refused. */
  refused(error) {
    this.error =
      typeof error === "object" && error !== null
        ? error
        : { message: String(error) };
    this.review = null;
    this.notify(`${this.error.message} Your unapplied changes are kept.`);
  }

  /** captures the exact source revision and operations for a later confirmed write. */
  async check(reload = false) {
    if (
      this.reason ||
      this.busy ||
      this.review ||
      (!reload && (this.fileReason || !this.dirty))
    )
      return;
    if (reload && this.dirty && !this.reloadAsk) {
      this.reloadAsk = true;
      this.changed("keep-editing");
      return;
    }
    this.reloadAsk = false;
    this.applyResult = null;
    this.busy = true;
    this.error = null;
    this.problemsInResult = false;
    this.notify("Checking changes. Nothing is written yet.");
    try {
      if (reload) {
        const read = await this.invoke("configuration", {
          request: { kind: "read" },
        });
        this.loaded(read.value);
      }
      const request = reload
        ? { kind: "reload", revision: this.document.revision, check: true }
        : {
            kind: "edit",
            revision: this.document.revision,
            edits: this.edits(),
            check: true,
          };
      this.problemsInResult = true;
      const answer = await this.invoke("configuration", { request });
      if (this.reason || answer.value.applies === this.access.stopped) {
        this.notify(STATE_CHANGED);
        return;
      }
      this.review = { request, change: answer.value, remove: new Set() };
      this.opener = reload ? "reload-config" : "check-config";
      this.notify(
        "Checked. Review each pool's effect before applying.",
        "cancel-config",
      );
    } catch (error) {
      this.refused(error);
    } finally {
      this.busy = false;
      this.changed();
    }
  }

  /**
   * cancels the active question, result or review, returning focus to its opener. The
   * result and the review show only on the Pools tab, so Escape cancels them only
   * while that panel is on show; the question bar shows on every tab.
   */
  cancel({ panelShown = true } = {}) {
    if (this.busy) return false;
    if (this.readAsk) {
      this.readAsk = false;
      this.changed("read-again-config");
      return true;
    }
    if (this.reloadAsk) {
      this.reloadAsk = false;
      this.changed("reload-config");
      return true;
    }
    if (!panelShown) return false;
    if (this.applyResult) {
      this.applyResult = null;
      this.changed("heading");
      return true;
    }
    if (!this.review) return false;
    this.review = null;
    this.notify("Review cancelled. Nothing was applied.", this.opener);
    return true;
  }

  /** writes the checked request, then performs only separately chosen token removals. */
  async apply() {
    if (!this.review || this.reason || this.busy) return;
    const review = this.review;
    let saved = false;
    this.busy = true;
    this.changed();
    try {
      const checked = await this.invoke("configuration", {
        request: review.request,
      });
      if (
        this.review !== review ||
        this.reason ||
        checked.value.applies === this.access.stopped
      ) {
        this.review = null;
        this.applyResult = "Configuration not applied";
        this.notify(STATE_CHANGED);
        return;
      }
      if (JSON.stringify(checked.value) !== JSON.stringify(review.change)) {
        this.review = {
          request: review.request,
          change: checked.value,
          remove: new Set(),
        };
        this.notify(
          "The effects changed. Review them again before applying. Nothing was written.",
          "cancel-config",
        );
        return;
      }
      await this.invoke("configuration", {
        request: { ...review.request, check: false },
      });
      saved = true;
      this.applyResult = "Configuration saved";
      this.review = null;
      // the write succeeded even if the following read fails: the old draft is not
      // offered for another write under its now-stale revision.
      this.document = null;
      this.host = null;
      this.pools = [];
      this.changed();
      for (const repository of review.remove)
        await this.removeToken(repository);
      const answer = await this.invoke("configuration", {
        request: { kind: "read" },
      });
      this.loaded(answer.value);
      this.notify(
        review.change.applies
          ? "Saved. Marshal is applying the change without restarting its agent."
          : "Saved. The changes take effect at the next Start All.",
      );
    } catch (error) {
      if (saved) {
        this.error = null;
        this.notify(
          `Saved, but config.toml could not be read again: ${error?.message ?? String(error)}. Read Configuration to continue.`,
        );
      } else {
        this.applyResult = "Configuration not applied";
        this.refused(error);
      }
    } finally {
      this.busy = false;
      this.changed();
    }
  }

  /** a retained removal choice cannot overlap a configuration operation. */
  async retryToken(repository) {
    if (
      this.locked ||
      this.reason ||
      this.fileReason ||
      this.tokenRemovals.get(repository)?.state !== "failed"
    )
      return;
    this.busy = true;
    this.changed();
    try {
      await this.removeToken(repository);
    } finally {
      this.busy = false;
      this.changed();
    }
  }

  /** a failed store removal is retryable independently of the already saved edit. */
  async removeToken(repository) {
    try {
      await this.invoke("configuration", {
        request: { kind: "remove_released_token", repository },
      });
      this.tokenRemovals.set(repository, { state: "removed" });
      this.announce([`The stored token for ${repository} was removed.`]);
    } catch (error) {
      const cause = error?.message ?? String(error);
      this.tokenRemovals.set(repository, { state: "failed", cause });
      this.announce([`The token for ${repository} was not removed: ${cause}`]);
    }
    this.changed();
  }
}
