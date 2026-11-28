// shaped configuration answers at the Tauri command boundary. This fixture
// never connects to an agent, token store or host configuration file.

/** installs the command/event stand-in before the bundled main module starts. */
export function bridge({ views, scenario }) {
  const callbacks = new Map();
  let revision = 0;
  let current = structuredClone(
    views[{ stopped: "stopped", settings: "settings" }[scenario] ?? "running"],
  );
  const document = {
    revision: "file-fixture",
    status: "valid",
    settings: {
      routing_label: "axpc",
      image_name: "actions-runner:local",
      budget: { cpus: 12, memory_gb: 48, disk_gb: 240 },
    },
    pools: [
      {
        name: "app",
        repository: "owner/app",
        runners: 2,
        labels: ["linux", "x64", "axpc"],
        volumes: [{ suffix: "cache", mount_path: "/home/runner/.cache" }],
      },
      { name: "site", repository: "owner/site", runners: 3 },
      { name: "docs", repository: "owner/docs", runners: 1 },
    ],
    problems: [],
  };
  if (scenario === "invalid") {
    document.status = "invalid";
    document.problems = [
      { path: "pool[0].runners", line: 9, message: "must be at most 64" },
    ];
    document.pools[0].runners = 80;
  }
  if (scenario === "omitted") {
    document.status = "invalid";
    delete document.pools[0].labels;
    document.problems = ["labels", "diagnostics", "memory_gb"].map((key) => ({
      path: `pool[0].${key}`,
      line: 10,
      message: "must have the correct type",
    }));
  }
  if (["missing", "unusable", "too-large"].includes(scenario)) {
    document.status =
      scenario === "too-large"
        ? "valid"
        : scenario === "missing"
          ? "missing"
          : "unreadable";
    document.too_large = scenario === "too-large";
    document.pools = [];
    document.problems = [
      {
        path: "config.toml",
        message: "Fix or restore config.toml in a text editor.",
      },
    ];
  }
  const shown = () => ({ view: current, revision: ++revision, announce: [] });
  const state = (window.__fixture = {
    calls: [],
    scenario,
    hold: scenario === "checking",
    stopped: scenario === "stopped",
    hasRunners: true,
    emit(kind) {
      current = structuredClone(views[kind]);
      state.stopped = kind === "stopped";
      callbacks.get("marshal:view")?.({ payload: shown() });
    },
  });
  window.__TAURI__ = {
    event: {
      listen: async (name, callback) => {
        callbacks.set(name, callback);
        return () => {};
      },
    },
    core: {
      invoke: async (command, args) => {
        if (command === "read_view") return shown();
        state.calls.push({ command, ...args });
        if (command === "start_all") {
          state.emit("running");
          return;
        }
        if (command !== "configuration") return;
        const { request } = args;
        if (request.kind === "read") {
          if (
            state.failReadAfterWrite &&
            document.revision === "written-fixture"
          )
            throw { message: "Lost contact." };
          return { kind: "read", value: structuredClone(document) };
        }
        if (
          state.hold ||
          (state.holdWrite && request.check === false) ||
          (state.holdRemoval && request.kind === "remove_released_token")
        )
          await new Promise((resolve) => {
            state.release = resolve;
          });
        if (request.kind === "remove_released_token") {
          if (state.refuseRemoval) throw { message: "Access refused." };
          return { kind: "token_removed" };
        }
        const failures = {
          refused: {
            code: "config_invalid",
            message: "Fix the marked field.",
            problems: [
              {
                path: "pool[0].runners",
                line: 9,
                message: "must be at most 64",
              },
            ],
          },
          changed: {
            code: "config_changed",
            message: "config.toml changed elsewhere. Read it again.",
          },
          unwritable: {
            code: "config_unwritable",
            message: "config.toml cannot be replaced (access_refused).",
          },
        };
        if (request.check && state.refuseCheck) throw failures.changed;
        if (
          request.check === false &&
          (state.refuseWrite || scenario === "unwritable")
        )
          throw failures.unwritable;
        if (scenario !== "unwritable" && failures[scenario])
          throw failures[scenario];
        if (!request.check) {
          document.revision = "written-fixture";
          for (const edit of request.edits ?? []) {
            const pool = /^pool\[(\d+)\]$/.exec(edit.path ?? "");
            if (edit.op === "remove_entry" && pool)
              document.pools.splice(Number(pool[1]), 1);
            if (edit.op === "add_entry" && edit.array === "pool")
              document.pools.push(structuredClone(edit.values));
          }
        }
        return {
          kind: "change",
          value: {
            revision: "checked-fixture",
            applies: !state.stopped,
            effects: [
              {
                pool: "app",
                change: "restarted",
                runners_before: 2,
                runners_after: 2,
                stops_runners: !state.stopped && state.hasRunners,
              },
              {
                pool: "site",
                change: scenario === "review-tokens" ? "removed" : "unchanged",
                runners_before: 3,
                runners_after: scenario === "review-tokens" ? 0 : 3,
                stops_runners:
                  scenario === "review-tokens" &&
                  !state.stopped &&
                  state.hasRunners,
              },
              {
                pool: "docs",
                change: "unchanged",
                runners_before: 1,
                runners_after: 1,
                stops_runners: false,
              },
            ],
            repositories_released:
              scenario === "review-tokens" ? ["owner/site"] : [],
          },
        };
      },
    },
  };
}
