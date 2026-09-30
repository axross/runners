import { execFile } from "node:child_process";
import { realpath } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";
import type { PluginAPI, PluginLogger, ToolResultEvent } from "@ampcode/plugin";

export const description =
  "Formats the repository after a file-changing tool call, then lints pending changes before Amp finishes a successful turn.";

const execFileAsync = promisify(execFile);
const FORMAT_EXTENSIONS = new Set([
  ".md",
  ".json",
  ".jsonc",
  ".yaml",
  ".yml",
  ".ts",
  ".js",
  ".mjs",
]);
const CONTINUATION_MARKER = "[runners-quality-hooks:lint-follow-up]";
const OUTPUT_TAIL_LENGTH = 8_000;

export interface QualityHooksRuntime {
  realpath(candidate: string): Promise<string>;
  run(
    command: string,
    args: string[],
    repositoryRoot: string,
  ): Promise<{ stdout: string; stderr: string }>;
}

let qualityQueue = Promise.resolve();

function serializeQualityWork<T>(work: () => Promise<T>): Promise<T> {
  const result = qualityQueue.then(work, work);
  qualityQueue = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}

async function run(
  command: string,
  args: string[],
  repositoryRoot: string,
): Promise<{ stdout: string; stderr: string }> {
  return execFileAsync(command, args, {
    cwd: repositoryRoot,
    encoding: "utf8",
    maxBuffer: 10 * 1024 * 1024,
    env: {
      ...process.env,
      PATH: `${process.env.HOME}/.local/bin:${process.env.PATH}`,
    },
  });
}

const defaultRuntime: QualityHooksRuntime = { realpath, run };

function outputTail(error: unknown): string {
  const candidate = error as {
    stdout?: unknown;
    stderr?: unknown;
    message?: unknown;
  };
  const output = [candidate.stdout, candidate.stderr, candidate.message]
    .filter(
      (value): value is string => typeof value === "string" && value.length > 0,
    )
    .join("\n")
    .trim();
  return (
    output.slice(-OUTPUT_TAIL_LENGTH) || "The command failed without output."
  );
}

async function repositoryPath(
  candidate: string,
  repositoryRoot: string,
  runtime: QualityHooksRuntime,
): Promise<string | null> {
  try {
    const resolved = await runtime.realpath(candidate);
    const relative = path.relative(repositoryRoot, resolved);
    if (
      relative === "" ||
      relative.startsWith(`..${path.sep}`) ||
      path.isAbsolute(relative)
    ) {
      return null;
    }
    return relative;
  } catch {
    return null;
  }
}

async function modifiedProjectFiles(
  amp: PluginAPI,
  event: ToolResultEvent,
  repositoryRoot: string,
  runtime: QualityHooksRuntime,
): Promise<string[]> {
  const uris = amp.helpers.filesModifiedByToolCall(event) ?? [];
  const files = await Promise.all(
    uris.map((uri) =>
      repositoryPath(amp.helpers.filePathFromURI(uri), repositoryRoot, runtime),
    ),
  );
  return [...new Set(files.filter((file): file is string => file !== null))];
}

async function hasPendingChanges(
  repositoryRoot: string,
  runtime: QualityHooksRuntime,
): Promise<boolean> {
  const status = await runtime.run(
    "git",
    ["status", "--porcelain"],
    repositoryRoot,
  );
  if (status.stdout.trim() !== "") return true;

  try {
    let baseline: string;
    try {
      const upstream = await runtime.run(
        "git",
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"],
        repositoryRoot,
      );
      baseline = upstream.stdout.trim();
    } catch {
      const defaultRemoteBranch = await runtime.run(
        "git",
        ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        repositoryRoot,
      );
      baseline = defaultRemoteBranch.stdout.trim();
    }
    const changed = await runtime.run(
      "git",
      ["diff", "--name-only", `${baseline}...HEAD`],
      repositoryRoot,
    );
    return changed.stdout.trim() !== "";
  } catch {
    return false;
  }
}

function logFailure(
  logger: PluginLogger,
  command: string,
  error: unknown,
): void {
  logger.log(`${command} failed:\n${outputTail(error)}`);
}

export default function qualityHooks(
  amp: PluginAPI,
  runtime: QualityHooksRuntime = defaultRuntime,
): void {
  const rootURI = amp.system.workspaceRoot;
  const rootPath =
    rootURI === null ? process.cwd() : amp.helpers.filePathFromURI(rootURI);

  amp.on("tool.result", async (event, context) => {
    if (event.status !== "done") return;

    try {
      await serializeQualityWork(async () => {
        const repositoryRoot = await runtime.realpath(rootPath);
        const files = (
          await modifiedProjectFiles(amp, event, repositoryRoot, runtime)
        ).filter((file) => FORMAT_EXTENSIONS.has(path.extname(file)));
        if (files.length === 0) return;

        try {
          await runtime.run("mise", ["run", "format"], repositoryRoot);
        } catch (error) {
          logFailure(context.logger, "mise run format", error);
        }
      });
    } catch (error) {
      logFailure(context.logger, "quality repair", error);
    }
  });

  amp.on("agent.end", async (event, context) => {
    if (event.status !== "done" || event.message.includes(CONTINUATION_MARKER))
      return;

    try {
      return await serializeQualityWork(async () => {
        const repositoryRoot = await runtime.realpath(rootPath);
        if (!(await hasPendingChanges(repositoryRoot, runtime))) return;

        try {
          await runtime.run("mise", ["run", "lint"], repositoryRoot);
        } catch (error) {
          const output = outputTail(error);
          context.logger.log(`mise run lint failed:\n${output}`);
          return {
            action: "continue" as const,
            userMessage: `${CONTINUATION_MARKER}\nFix the lint errors below, then rerun the relevant checks before finishing. This automatic follow-up runs only once.\n\n${output}`,
          };
        }
      });
    } catch (error) {
      logFailure(context.logger, "pre-completion lint", error);
    }
  });
}
