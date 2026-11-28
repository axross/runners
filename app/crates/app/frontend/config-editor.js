// the window's unapplied configuration, kept in memory across hides. Checks and
// writes go only through the agent; a check's preview never replaces its source revision.

const SAVING_SETTING = "Unavailable while a setting is being saved.";

const STATE_CHANGED =
  "Marshal's state changed. Check and review again before applying. Your changes are kept.";

/** the codes of a call that was sent and may still have taken effect. */
const UNANSWERED = new Set(["not_answered", "lost_contact"]);

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
    case "texts": {
      const lines = text.split("\n").filter((line) => line !== "");
      return lines.length === 0 ? undefined : lines;
    }
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

/** an array of tables' operations: kept source entries at their file index, removals from
 * the end, then new entries appended at their resulting index. `nested` adds the
 * operations of what an entry holds, at the entry's path.
 * @throws the problem from an invalid numeric field.
 */
function entryEdits(rows, specs, array, nested = () => []) {
  const kept = rows.filter((row) => row.source !== null && !row.removed);
  const edits = kept.flatMap((row) => {
    const at = `${array}[${row.source}]`;
    return [...changedFields(row, specs, at), ...nested(row, at)];
  });
  for (const row of rows
    .filter((row) => row.source !== null && row.removed)
    .reverse()) {
    edits.push({ op: "remove_entry", path: `${array}[${row.source}]` });
  }
  rows
    .filter((row) => row.source === null && !row.removed)
    .forEach((row, n) => {
      const at = `${array}[${kept.length + n}]`;
      edits.push(
        { op: "add_entry", array, values: values(row, specs, at) },
        ...nested(row, at),
      );
    });
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
    this.writing = false;
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
    this.checked = null;
    this.settingCalls = 0;
    this.reads = 0;
    this.staleAfter = null;
  }

  /** view updates may invalidate consent, but must not replace the user's draft. */
  update(access) {
    this.access = access ?? null;
    if (
      this.review &&
      !this.writing &&
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
    return this.busy || this.review !== null || this.saving;
  }
  get lockReason() {
    if (this.busy) return "Waiting for the agent's answer.";
    if (this.saving) return SAVING_SETTING;
    return this.review ? "Finish or cancel the review first." : null;
  }

  /** whether a setting change from the Settings tab is waiting for its answer. */
  get saving() {
    return this.settingCalls > 0;
  }
  /** the reason the controls that read, check or apply give while a setting is saved. */
  get savingReason() {
    return this.saving ? SAVING_SETTING : null;
  }

  /**
   * keeps the editor's controls unavailable while `send` runs a setting change, then marks
   * the editor's read stale: the change may have moved config.toml's revision, and the
   * page is not told whether it did.
   */
  async whileSettingSaves(send) {
    this.settingCalls += 1;
    this.changed();
    try {
      await send();
    } finally {
      this.settingCalls -= 1;
      this.staleAfter = this.reads;
      this.changed();
    }
  }

  /** whether no read has begun since the last setting change returned. */
  get stale() {
    return this.staleAfter !== null;
  }

  /**
   * why the Settings checkboxes cannot be changed, or null. The editor is offered when the
   * agent sends its access; a form is shown only while the file is editable, but unapplied
   * changes in a hidden form still count.
   */
  get settingsReason() {
    if (!this.access) return null;
    if (this.busy)
      return "Settings cannot be changed while Marshal is reading, checking or applying the configuration.";
    if (this.review)
      return "Settings cannot be changed while a configuration change is being reviewed. Finish or cancel the review on the Pools tab first.";
    const formShown =
      !this.fileReason && (this.editing !== null || this.hostOpen);
    if (formShown || this.dirty || this.readAsk || this.reloadAsk)
      return "Settings cannot be changed while the configuration is open for editing. Apply, discard or close your changes first.";
    return null;
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
    return (
      Boolean(this.host) &&
      (this.rowChanged(this.host) ||
        this.pools.some((p) => this.poolChanged(p)))
    );
  }

  /** whether a row is new, removed or has a field that differs from the file's. */
  rowChanged(row) {
    return (
      row.removed ||
      row.source === null ||
      Object.keys(row.fields).some(
        (key) => row.fields[key] !== row.original[key],
      )
    );
  }

  /** whether a pool or any of its volumes changed. */
  poolChanged(pool) {
    return (
      this.rowChanged(pool) || pool.volumes.some((v) => this.rowChanged(v))
    );
  }

  /**
   * the prefix a pool's fields show, and the one the agent's problems name them by. before
   * a check both are the pool's place in the file; after one, problems follow the checked
   * result, so `matched` keeps that order however the draft changes and is null for a
   * pool the result lacks.
   */
  problemPrefixes(pool) {
    const live = this.pools.filter((p) => !p.removed).indexOf(pool);
    if (!this.problemsInResult) {
      const prefix = `pool[${pool.source ?? live}]`;
      return { shown: prefix, matched: prefix };
    }
    const checked = this.checked.pools.indexOf(pool);
    return {
      shown: `pool[${live}]`,
      matched: checked < 0 ? null : `pool[${checked}]`,
    };
  }

  /** a volume's prefixes, on the same terms as its pool's. */
  volumePrefixes(pool, volume) {
    const pools = this.problemPrefixes(pool);
    const live = pool.volumes.filter((v) => !v.removed).indexOf(volume);
    if (!this.problemsInResult) {
      const prefix = `${pools.shown}.volumes[${volume.source ?? live}]`;
      return { shown: prefix, matched: prefix };
    }
    const checked = this.checked.volumes.get(pool)?.indexOf(volume) ?? -1;
    return {
      shown: `${pools.shown}.volumes[${live}]`,
      matched:
        pools.matched === null || checked < 0
          ? null
          : `${pools.matched}.volumes[${checked}]`,
    };
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

  /**
   * asks the agent for config.toml. A read that began after the last setting change
   * returned, and is answered, ends the stale state; one already out when it returned does not.
   */
  async fetchFile() {
    this.reads += 1;
    const began = this.reads;
    const answer = await this.invoke("configuration", {
      request: { kind: "read" },
    });
    if (this.staleAfter !== null && began > this.staleAfter)
      this.staleAfter = null;
    return answer;
  }

  /**
   * asks before replacing a changed draft; a failed read retains the old draft. Nothing here
   * waits for a setting change: the editor's first read starts when the view first offers it.
   */
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
      const answer = await this.fetchFile();
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
    return [
      ...changedFields(this.host, this.access.settings, ""),
      ...entryEdits(this.pools, this.access.pool, "pool", (pool, at) =>
        entryEdits(pool.volumes, this.access.volume, `${at}.volumes`),
      ),
    ];
  }

  set(row, key, value) {
    if (this.locked) return;
    this.applyResult = null;
    row.fields[key] = value;
    this.changed();
  }

  /** why a form cannot be opened: the editor is held or, once stale, a read cannot be made. */
  get openReason() {
    return (
      this.lockReason ??
      this.fileReason ??
      (this.stale ? this.access.read_reason : null)
    );
  }

  /** opens a pool's form, reading config.toml again first when a setting change left the read stale. */
  async openPool(pool) {
    this.applyResult = null;
    let target = pool;
    if (this.stale) {
      await this.read();
      if (this.stale || this.fileReason) return;
      target = this.pools.find(
        (p) => p.source !== null && p.original.name === pool.original.name,
      );
      if (!target) return;
    }
    this.editing = target.id;
    this.changed(`edit-heading:${target.id}`);
  }

  /** opens the Host Settings form, or closes it; it reads config.toml first when none was read or the read is stale. */
  async toggleHost() {
    this.applyResult = null;
    if (!this.hostOpen && (!this.document || this.stale)) await this.read();
    this.hostOpen = this.hostOpen ? false : !this.stale && !this.fileReason;
    this.changed();
  }

  /** the new-pool form, after a read of config.toml when a setting change left the last one stale. */
  async openNewPool() {
    if (this.stale) {
      await this.read();
      if (this.stale || this.fileReason) return;
    }
    this.addPool();
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

  /** undoes a pool's removal mark, returning focus to the restored card. */
  keepPool(pool) {
    if (this.locked) return;
    this.applyResult = null;
    pool.removed = false;
    this.notify(
      "Pool kept. It is no longer marked for removal.",
      `edit-heading:${pool.id}`,
    );
  }

  /** drops a new volume, or marks a stored one for removal until Apply. */
  removeVolume(pool, volume) {
    if (this.locked) return;
    this.applyResult = null;
    if (volume.source === null)
      pool.volumes = pool.volumes.filter((v) => v !== volume);
    else volume.removed = true;
    this.changed(`edit-heading:${pool.id}`);
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

  /** a write the agent may have applied: the draft stays, and config.toml is read again first. */
  unconfirmed() {
    this.error = null;
    this.review = null;
    this.applyResult = "Configuration may have been saved";
    this.notify(
      "The agent did not confirm the write, so config.toml may have changed. Read config.toml again before editing. Your unapplied changes are kept.",
    );
  }

  /** captures the exact source revision and operations for a later confirmed write. */
  async check(reload = false) {
    if (
      this.reason ||
      this.busy ||
      this.saving ||
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
        const read = await this.fetchFile();
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
      const kept = this.pools.filter((p) => !p.removed);
      this.checked = {
        pools: kept,
        volumes: new Map(
          kept.map((p) => [p, p.volumes.filter((v) => !v.removed)]),
        ),
      };
      this.problemsInResult = true;
      const answer = await this.invoke("configuration", { request });
      if (this.reason || answer.value.applies === this.access.stopped) {
        this.problemsInResult = false;
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
      if (!error?.problems) this.problemsInResult = false;
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
    if (!this.review || this.reason || this.busy || this.saving) return;
    const review = this.review;
    let saved = false;
    let sent = false;
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
      sent = true;
      this.writing = true;
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
      const answer = await this.fetchFile();
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
          `Saved, but config.toml could not be read again. ${error?.message ?? String(error)} Read Configuration to continue.`,
        );
      } else if (sent && UNANSWERED.has(error?.code)) {
        this.unconfirmed();
      } else {
        this.applyResult = "Configuration not applied";
        this.refused(error);
      }
    } finally {
      this.busy = false;
      this.writing = false;
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
      if (UNANSWERED.has(error?.code)) {
        this.tokenRemovals.set(repository, { state: "unanswered" });
        this.announce([
          `The agent has not answered about removing the token for ${repository}. It may still be stored.`,
        ]);
      } else {
        const cause = error?.message ?? String(error);
        this.tokenRemovals.set(repository, { state: "failed", cause });
        this.announce([
          `The token for ${repository} was not removed: ${cause}`,
        ]);
      }
    }
    this.changed();
  }
}
