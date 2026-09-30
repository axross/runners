import assert from "node:assert/strict";
import { test } from "node:test";
import qualityHooks, {
  type QualityHooksRuntime,
} from "../plugins/quality-hooks.ts";

type Handler = (event: unknown, context: unknown) => Promise<unknown>;

const ROOT = "/repo";

function setup(options: {
  changedFiles?: string[];
  status?: string;
  failing?: (command: string, args: string[]) => Error | undefined;
  unresolvedBaseline?: boolean;
}) {
  const handlers: Record<string, Handler> = {};
  const calls: string[][] = [];
  const logs: string[] = [];

  const amp = {
    system: { workspaceRoot: `file://${ROOT}` },
    helpers: {
      filePathFromURI: (uri: string) => uri.replace("file://", ""),
      filesModifiedByToolCall: () =>
        (options.changedFiles ?? []).map((file) => `file://${ROOT}/${file}`),
    },
    on: (name: string, handler: Handler) => {
      handlers[name] = handler;
    },
  };

  const runtime: QualityHooksRuntime = {
    realpath: async (candidate) => candidate,
    run: async (command, args) => {
      calls.push([command, ...args]);
      const failure = options.failing?.(command, args);
      if (failure) throw failure;
      if (command === "git" && args[0] === "status") {
        return { stdout: options.status ?? "", stderr: "" };
      }
      if (
        options.unresolvedBaseline &&
        command === "git" &&
        (args[0] === "rev-parse" || args[0] === "symbolic-ref")
      ) {
        throw new Error("no baseline");
      }
      return { stdout: "", stderr: "" };
    },
  };

  // The plugin's PluginAPI type comes from a package that is not installed here,
  // and Node strips it without resolving it, so a stub is typed loosely.
  qualityHooks(amp as never, runtime);
  const context = { logger: { log: (message: string) => logs.push(message) } };
  return { handlers, calls, logs, context };
}

test("formats after a successful tool result that changed a formatted file", async () => {
  const { handlers, calls, context } = setup({ changedFiles: ["README.md"] });
  await handlers["tool.result"]!({ status: "done" }, context);
  assert.deepEqual(calls, [["mise", "run", "format"]]);
});

test("ignores a failed tool result and files that are not formatted", async () => {
  const failed = setup({ changedFiles: ["README.md"] });
  await failed.handlers["tool.result"]!({ status: "error" }, failed.context);
  assert.deepEqual(failed.calls, []);

  const other = setup({ changedFiles: ["image.png"] });
  await other.handlers["tool.result"]!({ status: "done" }, other.context);
  assert.deepEqual(other.calls, []);
});

test("lints only when changes are pending", async () => {
  const clean = setup({});
  const cleanResult = await clean.handlers["agent.end"]!(
    { status: "done", message: "" },
    clean.context,
  );
  assert.equal(cleanResult, undefined);
  assert.equal(
    clean.calls.some((call) => call[0] === "mise"),
    false,
  );

  const dirty = setup({ status: " M README.md\n" });
  await dirty.handlers["agent.end"]!(
    { status: "done", message: "" },
    dirty.context,
  );
  assert.deepEqual(
    dirty.calls.filter((call) => call[0] === "mise"),
    [["mise", "run", "lint"]],
  );
});

test("a lint failure continues once and the marker stops a second follow-up", async () => {
  const { handlers, calls, context } = setup({
    status: " M README.md\n",
    failing: (command, args) =>
      command === "mise" && args[1] === "lint"
        ? Object.assign(new Error("failed"), { stdout: "lint broke" })
        : undefined,
  });

  const result = (await handlers["agent.end"]!(
    { status: "done", message: "" },
    context,
  )) as { action: string; userMessage: string };
  assert.equal(result.action, "continue");
  assert.match(result.userMessage, /lint broke/);

  const marker = result.userMessage.split("\n")[0]!;
  const before = calls.length;
  const second = await handlers["agent.end"]!(
    { status: "done", message: marker },
    context,
  );
  assert.equal(second, undefined);
  assert.equal(calls.length, before);
});

test("falls back to origin/main and logs when no baseline resolves", async () => {
  const { handlers, calls, logs, context } = setup({
    unresolvedBaseline: true,
  });
  await handlers["agent.end"]!({ status: "done", message: "" }, context);
  assert.ok(
    calls.some(
      (call) => call[0] === "git" && call.includes("origin/main...HEAD"),
    ),
  );
  assert.ok(logs.some((line) => line.includes("origin/main")));
});
