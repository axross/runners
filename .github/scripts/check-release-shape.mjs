// fails unless the release workflows keep the shape docs/conventions/security.md
// records: which job may declare the `release` Environment and read the signing
// secrets, what that job may hold and run, how the signing jobs chain to the
// attest and release jobs, and that no job can bypass a failed signing step.
//
//   node .github/scripts/check-release-shape.mjs [root]
//
// `root` is the directory holding `.github/`; it defaults to the repository the
// script sits in, and the test points it at altered copies. the check reads the
// workflows with workflow-reader.mjs and the scripts as text, using only Node.
// it guards against mistakes: a pull request can edit it, so it is no control
// against a hostile one. what keeps pull requests away from the Environment is
// the maintainer's branch rule and the fact that release.yaml runs only on
// dispatch. a later signing job or step (W4, M2) is added to this check in its
// own pull request, under REVIEW.md's release lens.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { asList, readWorkflow, text } from "./workflow-reader.mjs";

const root = resolve(
  process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), "..", ".."),
);
const problems = [];
const problem = (message) => problems.push(message);

const GUARD =
  "github.ref == 'refs/heads/main' && needs.plan.outputs.version != ''";
const SIGNING_ENVIRONMENT = "release";
const SIGNING_JOB = "sign-macos";
const NEW_JOBS = ["sign-macos", "verify-macos", "assemble-signed"];
const SIGNING_ACTIONS = [
  "actions/download-artifact",
  "actions/upload-artifact",
];
const BIN_TOOLS = [
  "security",
  "codesign",
  "pkgbuild",
  "openssl",
  "plutil",
  "shasum",
  "xcrun",
];
const SBIN_TOOLS = ["pkgutil"];
const XCRUN_TOOLS = ["notarytool", "stapler"];
const PACKAGE_MANAGERS = [
  "npm",
  "npx",
  "brew",
  "pip",
  "pip3",
  "cargo",
  "mise",
  "gem",
  "yarn",
  "pnpm",
  "apt",
  "apt-get",
  "bundle",
];
const FETCHERS = ["curl", "wget"];
const INTERPRETERS = [
  "bash",
  "sh",
  "zsh",
  "dash",
  "source",
  ".",
  "eval",
  "exec",
  "node",
  "python",
  "python3",
  "ruby",
  "perl",
  "pwsh",
  "powershell",
];
// shell built-ins and the base utilities macOS ships, which sign-macos may call by name.
const ALLOWED_COMMANDS = new Set([
  ":",
  "[[",
  "echo",
  "printf",
  "cat",
  "rm",
  "mkdir",
  "cp",
  "mv",
  "chmod",
  "base64",
  "mktemp",
  "find",
  "sort",
  "awk",
  "sed",
  "tr",
  "grep",
  "wc",
  "head",
  "tail",
  "cut",
  "date",
  "true",
  "false",
  "exit",
  "return",
  "local",
  "read",
  "export",
  "unset",
  "set",
  "trap",
  "shift",
  "umask",
]);
const SHELL_KEYWORDS = new Set([
  "if",
  "then",
  "else",
  "elif",
  "do",
  "while",
  "until",
  "{",
  "time",
]);
const GUARD_CONTEXT = "every new job";
const LIMITS = { "sign-macos": 60, "verify-macos": 20, "assemble-signed": 10 };

function read(path) {
  return readFileSync(join(root, path), "utf8");
}

// ---- the workflows -------------------------------------------------------

const workflowDirectory = join(root, ".github", "workflows");
const workflows = {};
// GitHub runs a workflow file with either extension.
for (const file of readdirSync(workflowDirectory)
  .filter((name) => /\.ya?ml$/.test(name))
  .sort()) {
  try {
    workflows[file] = readWorkflow(join(workflowDirectory, file));
  } catch (error) {
    problem(`${file} cannot be read: ${error.message}`);
  }
}
const release = workflows["release.yaml"];
const releaseBuild = workflows["release-build.yaml"];
if (!release) problem("release.yaml is missing or unreadable");
if (!releaseBuild) problem("release-build.yaml is missing or unreadable");

const jobsOf = (doc) => Object.entries(doc?.jobs ?? {});
const stepsOf = (job) => (Array.isArray(job?.steps) ? job.steps : []);
const sameMap = (value, expected) => {
  const keys =
    value && typeof value === "object" ? Object.keys(value).sort() : [];
  const wanted = Object.keys(expected).sort();
  return (
    keys.length === wanted.length &&
    keys.every(
      (key, index) => key === wanted[index] && value[key] === expected[key],
    )
  );
};
const triggersOf = (doc) => {
  const on = doc?.on;
  if (typeof on === "string") return [on];
  if (Array.isArray(on)) return on;
  return Object.keys(on ?? {});
};

// ---- the Environment -----------------------------------------------------

function declaresEnvironment(doc) {
  return jobsOf(doc).some(
    ([, job]) =>
      "environment" in job ||
      stepsOf(job).some((step) => "environment" in step),
  );
}

for (const [file, doc] of Object.entries(workflows)) {
  for (const [id, job] of jobsOf(doc)) {
    const allowed =
      file === "release.yaml" &&
      id === SIGNING_JOB &&
      job.environment === SIGNING_ENVIRONMENT;
    if ("environment" in job && !allowed) {
      problem(
        `${file}: job ${id} declares an environment; only release.yaml's ${SIGNING_JOB} may, and only \`${SIGNING_ENVIRONMENT}\``,
      );
    }
    for (const step of stepsOf(job)) {
      if ("environment" in step)
        problem(`${file}: a step of job ${id} declares an environment`);
    }
  }
}

if (release) {
  const triggers = triggersOf(release);
  if (triggers.length !== 1 || triggers[0] !== "workflow_dispatch") {
    problem(
      `release.yaml is triggered by ${triggers.join(", ") || "nothing"}, not by workflow_dispatch alone`,
    );
  }
}

// a workflow that a pull request triggers must not call a job that declares an
// Environment, directly or through another called workflow.
function reachesEnvironment(file, seen = new Set()) {
  if (seen.has(file)) return false;
  seen.add(file);
  const doc = workflows[file];
  if (!doc) return false;
  if (declaresEnvironment(doc)) return true;
  return jobsOf(doc).some(([, job]) => {
    const target = text(job.uses).replace(/^\.\/\.github\/workflows\//, "");
    return (
      text(job.uses).startsWith("./.github/workflows/") &&
      reachesEnvironment(target, seen)
    );
  });
}
for (const [file, doc] of Object.entries(workflows)) {
  const triggers = triggersOf(doc);
  if (
    !triggers.some(
      (name) => name === "pull_request" || name === "pull_request_target",
    )
  )
    continue;
  for (const [id, job] of jobsOf(doc)) {
    const uses = text(job.uses);
    if (uses === "") continue;
    if (!uses.startsWith("./.github/workflows/")) {
      problem(
        `${file}: job ${id} calls ${uses}, which this check cannot follow; a pull request's workflow may call only local workflows`,
      );
    } else if (reachesEnvironment(uses.replace("./.github/workflows/", ""))) {
      problem(
        `${file}: a pull request triggers job ${id}, which calls ${uses}, a workflow that declares an environment`,
      );
    }
  }
}

// ---- sign-macos ----------------------------------------------------------

const scriptsOf = (job) =>
  stepsOf(job).filter((step) => typeof step.run === "string");
const jobs = release ? (release.jobs ?? {}) : {};
const signing = jobs[SIGNING_JOB];

// a script's lines without comments or line continuations.
function codeLines(script) {
  return script
    .replace(/\\\n/g, " ")
    .split("\n")
    .filter((line) => !line.trimStart().startsWith("#"));
}

const TOOL_PATH = /^\$\{?SYSTEM_S?BIN\}?\/[A-Za-z0-9_]+$/;

// the script with the text of every quoted string blanked, so that a `;` or a
// tool's name inside a message is not read as a command. a `$(...)` inside a
// string is kept, and so is a string that is only a tool's path.
function skeleton(script) {
  const source = script.replace(/\\\n/g, " ");
  function scan(start, closer) {
    let out = "";
    let i = start;
    while (i < source.length) {
      const char = source[i];
      if (closer && char === ")") return [out, i + 1];
      if (char === "\\") {
        out += "xx";
        i += 2;
      } else if (char === "'") {
        const end = source.indexOf("'", i + 1);
        if (end === -1) throw new Error("an unterminated single quote");
        out += `'${"x".repeat(end - i - 1)}'`;
        i = end + 1;
      } else if (char === "$" && source[i + 1] === "'") {
        let j = i + 2;
        while (j < source.length && source[j] !== "'")
          j += source[j] === "\\" ? 2 : 1;
        out += `$'${"x".repeat(j - i - 2)}'`;
        i = j + 1;
      } else if (char === '"') {
        let j = i + 1;
        let inner = "";
        while (j < source.length && source[j] !== '"') {
          if (source[j] === "\\") {
            inner += "xx";
            j += 2;
          } else if (
            source[j] === "$" &&
            source[j + 1] === "(" &&
            source[j + 2] !== "("
          ) {
            const [sub, next] = scan(j + 2, ")");
            inner += `$(${sub})`;
            j = next;
          } else {
            inner += "x";
            j += 1;
          }
        }
        if (j >= source.length) throw new Error("an unterminated double quote");
        const raw = source.slice(i + 1, j);
        out += `"${TOOL_PATH.test(raw) ? raw : inner}"`;
        i = j + 1;
      } else if (source.startsWith("$((", i)) {
        const end = source.indexOf("))", i);
        if (end === -1) throw new Error("an unterminated arithmetic expansion");
        out += "0";
        i = end + 2;
      } else if (char === "$" && source[i + 1] === "(") {
        const [sub, next] = scan(i + 2, ")");
        out += `$(${sub})`;
        i = next;
      } else if (char === "#" && (i === 0 || /\s/.test(source[i - 1]))) {
        while (i < source.length && source[i] !== "\n") i += 1;
      } else {
        out += char;
        i += 1;
      }
    }
    return [out, i];
  }
  return scan(0, null)[0];
}

// every word a script uses as a command, and the functions it defines.
function commandWords(script) {
  let flat = skeleton(script);
  const functions = new Set();
  flat = flat.replace(/\[\[ .*? \]\]/g, "[[ ]]");
  flat = flat.replace(/^[ \t]*([A-Za-z_]\w*)\(\)[ \t]*\{/gm, (_, name) => {
    functions.add(name);
    return "";
  });
  flat = flat
    .replace(/[0-9]*[<>]+&[0-9-]+/g, " ")
    .replace(/[0-9]*(?:<<<|<<|>>|>|<)/g, " ");
  const words = [];
  for (const segment of flat.split(/\$\(|[;&|()\n]/)) {
    const tokens = segment.trim().split(/\s+/).filter(Boolean);
    let index = 0;
    for (;;) {
      const token = tokens[index];
      if (token === undefined) break;
      if (
        token === "!" ||
        SHELL_KEYWORDS.has(token) ||
        /^[A-Za-z_]\w*\+?=/.test(token)
      ) {
        index += 1;
      } else {
        break;
      }
    }
    const word = tokens[index];
    if (
      word === undefined ||
      word === "for" ||
      word === "}" ||
      /^["']?x*["']?$/.test(word)
    )
      continue;
    if (["fi", "done", "esac", "in"].includes(word)) continue;
    words.push(word);
  }
  return { words, functions };
}

function usedTools(script, variable) {
  const found = new Set();
  const pattern = new RegExp(`\\$\\{?${variable}\\}?/([A-Za-z0-9_]+)`, "g");
  for (const line of codeLines(script)) {
    for (const match of line.matchAll(pattern)) found.add(match[1]);
  }
  return found;
}

function checkSigningJob() {
  if (!signing) {
    problem(`release.yaml has no ${SIGNING_JOB} job`);
    return;
  }
  const where = `release.yaml ${SIGNING_JOB}`;
  if (!sameMap(signing.permissions, { contents: "read" })) {
    problem(`${where} must hold exactly \`contents: read\``);
  }
  if (signing.environment !== SIGNING_ENVIRONMENT) {
    problem(`${where} must declare \`environment: ${SIGNING_ENVIRONMENT}\``);
  }
  if (signing["runs-on"] !== "macos-latest")
    problem(`${where} must run on macos-latest`);

  const scripts = [];
  for (const step of stepsOf(signing)) {
    const label = text(step.name) || text(step.uses) || "a step";
    if (typeof step.uses === "string") {
      const action = step.uses.split("@")[0];
      if (action === "actions/checkout") {
        problem(`${where}: step "${label}" checks out the repository`);
      } else if (!SIGNING_ACTIONS.includes(action)) {
        problem(
          `${where}: step "${label}" uses ${step.uses}; only ${SIGNING_ACTIONS.join(" and ")} are allowed`,
        );
      } else if (!/@v\d+$/.test(step.uses)) {
        problem(`${where}: step "${label}" uses ${step.uses}, not a major tag`);
      }
    }
    if (typeof step.run === "string") {
      if (step.run.includes("${{"))
        problem(`${where}: step "${label}" expands \${{ }} inside run`);
      scripts.push({ label, script: step.run, step });
    }
  }

  const bin = new Set();
  const sbin = new Set();
  for (const { label, script } of scripts) {
    const first = script.split("\n")[0];
    if (!/^# sign-macos: [a-z]+$/.test(first)) {
      problem(
        `${where}: the script of step "${label}" does not start with a "# sign-macos: <name>" marker line`,
      );
    }
    for (const tool of usedTools(script, "SYSTEM_BIN")) bin.add(tool);
    for (const tool of usedTools(script, "SYSTEM_SBIN")) sbin.add(tool);

    // the stand-in prefixes: literal defaults, set by nothing else.
    for (const [variable, fallback] of [
      ["SYSTEM_BIN", "/usr/bin"],
      ["SYSTEM_SBIN", "/usr/sbin"],
    ]) {
      const uses = new RegExp(`\\$\\{?${variable}\\b`).test(script);
      const lines = codeLines(script).map((line) => line.trim());
      if (uses && !lines.includes(`: "\${${variable}:=${fallback}}"`)) {
        problem(
          `${where}: the script of step "${label}" uses ${variable} without the line : "\${${variable}:=${fallback}}"`,
        );
      }
      for (const match of script.matchAll(
        new RegExp(`\\$\\{${variable}[:=?+-]+([^}]*)\\}`, "g"),
      )) {
        if (match[0] !== `\${${variable}:=${fallback}}`) {
          problem(
            `${where}: the script of step "${label}" gives ${variable} a default other than ${fallback}: ${match[0]}`,
          );
        }
      }
      if (
        new RegExp(`\\b${variable}\\s*=`).test(script) ||
        new RegExp(`\\bexport\\s+${variable}\\b`).test(script)
      ) {
        problem(`${where}: the script of step "${label}" assigns ${variable}`);
      }
    }
    if (
      /\.github\/|GITHUB_WORKSPACE|github\.workspace/.test(
        codeLines(script).join("\n"),
      )
    ) {
      problem(
        `${where}: the script of step "${label}" refers to the repository's files`,
      );
    }

    // commands.
    let analysis;
    try {
      analysis = commandWords(script);
    } catch (error) {
      problem(
        `${where}: the script of step "${label}" cannot be analysed: ${error.message}`,
      );
      continue;
    }
    for (const word of analysis.words) {
      const bare = word.replaceAll('"', "");
      if (/^\$\{?SYSTEM_S?BIN\}?\//.test(bare)) continue;
      if (analysis.functions.has(bare) || ALLOWED_COMMANDS.has(bare)) continue;
      if (PACKAGE_MANAGERS.includes(bare)) {
        problem(`${where}: step "${label}" runs the package manager ${bare}`);
      } else if (FETCHERS.includes(bare)) {
        problem(
          `${where}: step "${label}" runs ${bare}; the job downloads nothing but its artifacts`,
        );
      } else if (BIN_TOOLS.includes(bare) || SBIN_TOOLS.includes(bare)) {
        problem(
          `${where}: step "${label}" calls ${bare} by name, not by its system path`,
        );
      } else if (
        INTERPRETERS.includes(bare) ||
        bare.startsWith("./") ||
        bare.startsWith("../") ||
        bare.includes(".github")
      ) {
        problem(
          `${where}: step "${label}" runs ${bare}, which could run a file from the repository`,
        );
      } else {
        problem(
          `${where}: step "${label}" runs ${bare}, which is neither a shell built-in, a base utility nor a tool at its system path`,
        );
      }
    }
  }
  // the only steps that may run after a failure.
  for (const step of stepsOf(signing)) {
    const condition = text(step.if);
    if (!/\b(always|failure|cancelled|success)\(\)/.test(condition)) continue;
    const isCleanup =
      typeof step.run === "string" &&
      step.run.split("\n")[0] === "# sign-macos: cleanup";
    const isLogUpload =
      typeof step.uses === "string" &&
      step.uses.startsWith("actions/upload-artifact@") &&
      step.with?.name === "notary-log-macos";
    if (!isCleanup && !isLogUpload) {
      problem(
        `${where}: the condition \`${condition}\` of step "${text(step.name)}" is allowed only on the clean-up and notary-log upload steps`,
      );
    }
  }
  // and those two must run after a failure, or a failed signing run keeps its
  // keychain and loses its log.
  const cleanup = stepsOf(signing).filter(
    (step) =>
      typeof step.run === "string" &&
      step.run.split("\n")[0] === "# sign-macos: cleanup",
  );
  const logUpload = stepsOf(signing).filter(
    (step) =>
      typeof step.uses === "string" &&
      step.uses.startsWith("actions/upload-artifact@") &&
      step.with?.name === "notary-log-macos",
  );
  if (cleanup.length !== 1 || cleanup[0].if !== "always()") {
    problem(
      `${where} must have exactly one clean-up step, with \`if: always()\``,
    );
  }
  if (logUpload.length !== 1 || logUpload[0].if !== "always()") {
    problem(
      `${where} must upload notary-log-macos once, with \`if: always()\``,
    );
  }
  for (const tool of bin) {
    if (!BIN_TOOLS.includes(tool))
      problem(
        `${where} calls $SYSTEM_BIN/${tool}, which is not one of ${BIN_TOOLS.join(", ")}`,
      );
  }
  for (const tool of sbin) {
    if (!SBIN_TOOLS.includes(tool))
      problem(
        `${where} calls $SYSTEM_SBIN/${tool}, which is not one of ${SBIN_TOOLS.join(", ")}`,
      );
  }
  // the check script names every tool the job calls.
  const check = scripts.find(
    (entry) => entry.script.split("\n")[0] === "# sign-macos: check",
  );
  if (!check) {
    problem(`${where} has no "# sign-macos: check" script`);
  } else {
    const checked = (name) =>
      new Set(
        (
          (check.script.match(new RegExp(`^\\s*${name}="([^"]*)"`, "m")) ??
            [])[1] ?? ""
        )
          .split(/\s+/)
          .filter(Boolean),
      );
    const checkedBin = checked("bin_tools");
    const checkedSbin = checked("sbin_tools");
    for (const tool of bin)
      if (!checkedBin.has(tool))
        problem(
          `${where}: the check script does not check that ${tool} exists`,
        );
    for (const tool of sbin)
      if (!checkedSbin.has(tool))
        problem(
          `${where}: the check script does not check that ${tool} exists`,
        );
    for (const tool of XCRUN_TOOLS)
      if (!check.script.includes(`for tool in ${XCRUN_TOOLS.join(" ")}`))
        problem(
          `${where}: the check script does not look for ${tool} with xcrun --find`,
        );
  }

  checkPackageArguments(where, scripts);
}

// ---- build-pkg.sh --------------------------------------------------------

// the arguments of the command that starts a line with `command`, with
// continuation lines joined.
function invocationOf(script, command) {
  const lines = script.split("\n");
  const start = lines.findIndex(
    (line) =>
      line.trimStart().startsWith(`${command} `) ||
      line.trim() === `${command} \\`,
  );
  if (start === -1) return null;
  let joined = "";
  for (let i = start; i < lines.length; i += 1) {
    const line = lines[i].trim();
    joined += ` ${line.replace(/\\$/, "")}`;
    if (!line.endsWith("\\")) break;
  }
  return joined.trim().split(/\s+/).slice(1);
}

function checkPackageArguments(where, scripts) {
  let build;
  try {
    build = read(".github/scripts/build-pkg.sh");
  } catch {
    problem("build-pkg.sh is missing");
    return;
  }
  const identifier = (build.match(/^identifier="([^"]+)"$/m) ?? [])[1];
  if (!identifier) {
    problem("build-pkg.sh sets no identifier");
    return;
  }
  for (const { label, script } of scripts) {
    for (const match of script.matchAll(/^\s*identifier="([^"]*)"\s*$/gm)) {
      if (match[1] !== identifier)
        problem(
          `${where}: step "${label}" sets identifier "${match[1]}", not build-pkg.sh's "${identifier}"`,
        );
    }
  }
  const layout = build
    .split("\n")
    .filter((line) =>
      /^(mkdir -p "\$\{stage\}|cp "\$binary" |chmod )/.test(line),
    );
  const pkgbuildArguments = invocationOf(build, "pkgbuild");
  if (layout.length !== 3 || !pkgbuildArguments) {
    problem(
      "build-pkg.sh no longer has the staging and pkgbuild lines that sign-macos is compared with",
    );
    return;
  }
  const packaging = scripts.find(
    (entry) => entry.script.split("\n")[0] === "# sign-macos: package",
  );
  if (!packaging) {
    problem(`${where} has no "# sign-macos: package" script`);
    return;
  }
  const lines = packaging.script.split("\n").map((line) => line.trim());
  const at = lines.indexOf(layout[0]);
  if (at === -1 || lines[at + 1] !== layout[1] || lines[at + 2] !== layout[2]) {
    problem(
      `${where}: the package script's staging lines differ from build-pkg.sh's:\n    ${layout.join("\n    ")}`,
    );
  }
  const signed = invocationOf(packaging.script, '"$SYSTEM_BIN/pkgbuild"');
  if (!signed) {
    problem(
      `${where}: the package script does not call "$SYSTEM_BIN/pkgbuild"`,
    );
    return;
  }
  const options = ["--sign", "--keychain"];
  const counts = options.map(
    (option) => signed.filter((argument) => argument === option).length,
  );
  if (counts.some((count) => count !== 1)) {
    problem(
      `${where}: the package script must pass --sign and --keychain to pkgbuild once each`,
    );
  }
  const rest = [];
  for (let i = 0; i < signed.length; i += 1) {
    if (options.includes(signed[i])) i += 1;
    else rest.push(signed[i]);
  }
  if (rest.join(" ") !== pkgbuildArguments.join(" ")) {
    problem(
      `${where}: pkgbuild's arguments differ from build-pkg.sh's apart from --sign and --keychain:\n    ${rest.join(" ")}\n    ${pkgbuildArguments.join(" ")}`,
    );
  }
}

// ---- the other new jobs, and the chain -----------------------------------

function checkNewJobs() {
  for (const id of NEW_JOBS) {
    const job = jobs[id];
    if (!job) {
      problem(`release.yaml has no ${id} job`);
      continue;
    }
    const where = `release.yaml ${id}`;
    if (!asList(job.needs).includes("plan"))
      problem(`${where} (${GUARD_CONTEXT}) must need plan directly`);
    if (text(job.if).trim() !== GUARD)
      problem(`${where} must carry \`if: ${GUARD}\``);
    if (id !== SIGNING_JOB) {
      if (!sameMap(job.permissions, { contents: "read" }))
        problem(`${where} must hold exactly \`contents: read\``);
      if ("environment" in job) problem(`${where} must declare no environment`);
    }
    if (!(
      Number(job["timeout-minutes"]) > 0 &&
      Number(job["timeout-minutes"]) <= LIMITS[id]
    )) {
      problem(`${where} must set timeout-minutes to at most ${LIMITS[id]}`);
    }
    if (id !== SIGNING_JOB) {
      for (const step of stepsOf(job)) {
        if (
          text(step.uses).startsWith("actions/checkout@") &&
          step.with?.["persist-credentials"] !== "false"
        ) {
          problem(
            `${where}: a checkout does not set persist-credentials: false`,
          );
        }
      }
    }
  }
  const needs = {
    "sign-macos": ["plan", "build"],
    "verify-macos": ["plan", "build", "sign-macos"],
    "assemble-signed": ["plan", "build", "sign-macos", "verify-macos"],
    attest: ["plan", "assemble-signed"],
    release: ["plan", "attest"],
  };
  for (const [id, wanted] of Object.entries(needs)) {
    const have = asList(jobs[id]?.needs);
    for (const name of wanted) {
      if (!have.includes(name))
        problem(`release.yaml ${id} must need ${name} directly`);
    }
  }
  if (
    jobs.attest &&
    !sameMap(jobs.attest.permissions, {
      "id-token": "write",
      attestations: "write",
    })
  ) {
    problem(
      "release.yaml attest must hold exactly `id-token: write` and `attestations: write`",
    );
  }
  for (const id of ["attest", "release"]) {
    const downloads = stepsOf(jobs[id]).filter((step) =>
      text(step.uses).startsWith("actions/download-artifact@"),
    );
    if (
      downloads.length !== 1 ||
      downloads[0].with?.name !== "marshal-release-signed"
    ) {
      problem(
        `release.yaml ${id} must download exactly marshal-release-signed`,
      );
    }
  }
  if (
    jobs.release &&
    stepsOf(jobs.release).some((step) =>
      JSON.stringify(step).includes("needs.build"),
    )
  ) {
    problem("release.yaml release must not read the build's artifact");
  }

  // no way past a failed job.
  for (const [id, job] of Object.entries(jobs)) {
    if ("continue-on-error" in job)
      problem(`release.yaml ${id} sets continue-on-error`);
    if (/\b(always|failure|cancelled)\(\)/.test(text(job.if))) {
      problem(
        `release.yaml ${id} has a job-level condition that can run it after a failure: ${job.if}`,
      );
    }
    for (const step of stepsOf(job)) {
      if ("continue-on-error" in step)
        problem(
          `release.yaml ${id}: step "${text(step.name)}" sets continue-on-error`,
        );
      if (
        id !== SIGNING_JOB &&
        /\b(always|failure|cancelled)\(\)/.test(text(step.if))
      ) {
        problem(
          `release.yaml ${id}: step "${text(step.name)}" can run after a failure`,
        );
      }
    }
  }
}

// ---- the tools the dry run measures --------------------------------------

function checkMeasuredTools() {
  const build = jobsOf(releaseBuild).find(([id]) => id === "build")?.[1];
  const step = stepsOf(build).find(
    (entry) =>
      typeof entry.run === "string" &&
      entry.run.includes("MEASURED: signing tools"),
  );
  if (!step) {
    problem(
      "release-build.yaml's build job has no step that prints the MEASURED: signing tools line",
    );
    return;
  }
  if (step.if !== "runner.os == 'macOS'")
    problem(
      "release-build.yaml's signing tools step must run on the macOS leg only",
    );
  if (!/exit 1/.test(step.run))
    problem(
      "release-build.yaml's signing tools step does not fail when a tool is missing",
    );
  if (
    "env" in step &&
    Object.keys(step.env ?? {}).some((key) => /SECRET|MACOS_/.test(key))
  ) {
    problem("release-build.yaml's signing tools step receives a signing value");
  }
  const listed = (name) =>
    new Set(
      (
        (step.run.match(new RegExp(`^\\s*${name}="([^"]*)"`, "m")) ?? [])[1] ??
        ""
      )
        .split(/\s+/)
        .filter(Boolean),
    );
  const measuredBin = listed("bin_tools");
  const measuredSbin = listed("sbin_tools");
  const needBin = new Set();
  const needSbin = new Set();
  const sources = [];
  for (const [id, job] of Object.entries(jobs)) {
    if (id === SIGNING_JOB || id === "verify-macos") {
      for (const entry of scriptsOf(job)) sources.push(entry.run);
    }
  }
  let testScript = null;
  try {
    testScript = read(".github/scripts/tests/test-signed-pkg.sh");
  } catch {
    problem("test-signed-pkg.sh is missing");
  }
  if (testScript) sources.push(testScript);
  for (const source of sources) {
    for (const tool of usedTools(source, "SYSTEM_BIN")) needBin.add(tool);
    for (const tool of usedTools(source, "SYSTEM_SBIN")) needSbin.add(tool);
  }
  for (const tool of needBin)
    if (!measuredBin.has(tool))
      problem(
        `release-build.yaml's signing tools step does not measure /usr/bin/${tool}`,
      );
  for (const tool of needSbin)
    if (!measuredSbin.has(tool))
      problem(
        `release-build.yaml's signing tools step does not measure /usr/sbin/${tool}`,
      );
  for (const tool of XCRUN_TOOLS) {
    if (!step.run.includes(`for tool in ${XCRUN_TOOLS.join(" ")}`))
      problem(
        `release-build.yaml's signing tools step does not look for ${tool} with xcrun --find`,
      );
  }
}

// ---- release-build.yaml's assemble ----------------------------------------

function checkBuildAssemble() {
  const assemble = jobsOf(releaseBuild).find(([id]) => id === "assemble")?.[1];
  if (!assemble) {
    problem("release-build.yaml has no assemble job");
    return;
  }
  for (const step of stepsOf(assemble)) {
    if (
      text(step.uses).startsWith("actions/checkout@") &&
      step.with?.["persist-credentials"] !== "false"
    ) {
      problem(
        "release-build.yaml assemble: a checkout does not set persist-credentials: false",
      );
    }
  }
  const scripts = scriptsOf(assemble).map((step) => step.run);
  if (
    !scripts.some((script) =>
      script.includes(".github/scripts/assemble-release.sh"),
    )
  ) {
    problem("release-build.yaml assemble does not call assemble-release.sh");
  }
  if (scripts.some((script) => /SHA256SUMS\s*$|>\s*SHA256SUMS/m.test(script))) {
    problem(
      "release-build.yaml assemble writes SHA256SUMS itself; assemble-release.sh is the only writer",
    );
  }
}

// the stand-in prefixes are the scripts' own defaults. no env of any workflow, job
// or step may set them, or the tools could be called from anywhere.
function keyPaths(value, pattern, path = "") {
  if (Array.isArray(value)) {
    return value.flatMap((item, index) =>
      keyPaths(item, pattern, `${path}[${index}]`),
    );
  }
  if (value && typeof value === "object") {
    return Object.entries(value).flatMap(([key, item]) =>
      pattern.test(key)
        ? [`${path}.${key}`]
        : keyPaths(item, pattern, `${path}.${key}`),
    );
  }
  return [];
}
for (const [file, doc] of Object.entries(workflows)) {
  for (const path of keyPaths(doc, /^SYSTEM_S?BIN$/)) {
    problem(
      `${file}: ${path} sets SYSTEM_BIN or SYSTEM_SBIN; only the stand-in test may`,
    );
  }
}

if (release && releaseBuild) {
  checkSigningJob();
  checkNewJobs();
  checkMeasuredTools();
  checkBuildAssemble();
}
if (existsSync(join(root, ".github", "scripts", "build-pkg.sh")) === false)
  problem("build-pkg.sh is missing");

if (problems.length > 0) {
  for (const message of problems) console.error(`FAIL: ${message}`);
  console.error(
    `${problems.length} problem(s) in the release workflows' shape.`,
  );
  process.exit(1);
}
console.log(
  "The release workflows have the shape the release record describes.",
);
