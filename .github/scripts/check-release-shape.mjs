// fails unless the release workflows keep the shape docs/conventions/security.md
// records: that only release.yaml's sign-update job declares the `release`
// Environment, and only its sign step reads a secret of release.yaml or
// release-build.yaml; what sign-update may hold and run; how the jobs of
// release.yaml chain from plan through update-tool, sign-update and
// assemble-signed to attest and release; and that no job can bypass a failed one.
//
//   node .github/scripts/check-release-shape.mjs [root]
//
// `root` is the directory holding `.github/`; it defaults to the repository the
// script sits in, and the test points it at altered copies. the check reads the
// workflows with workflow-reader.mjs and the scripts as text, using only Node.
// it guards against mistakes: a pull request can edit it, so it is no control
// against a hostile one. what keeps pull requests away from the Environment is
// the maintainer's branch rule and the fact that release.yaml runs only on
// dispatch. it reads the commands a signing script runs, a trap's body
// included, but not what its commands are told to do, so these can still run
// code it does not see:
//   - the programs of awk and sed, and the operands of a `[[ ]]` test, of which
//     it reads only the command substitutions;
//   - an option or variable name built at run time, such as `find "$option"` or
//     `read -r "$name"` and `printf -v "$name"`;
//   - an assignment through `${PATH:=...}`, and any environment variable other
//     than PATH, BASH_ENV and ENV.
// it also does not see a write to SHA256SUMS in release-build.yaml's assemble
// job other than by a redirection, tee, cp or mv, or a step condition in attest
// or release that skips a step, such as `if: false`. M2's signing job is added
// to this check in its own pull request, under REVIEW.md's release lens.
import { readdirSync } from "node:fs";
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
const SIGNING_JOB = "sign-update";
// the marker of the one step that reads the update-feed key, and the variables
// it reads it into.
const SIGNING_STEP = `# ${SIGNING_JOB}: sign`;
const SIGNING_SECRETS = {
  TAURI_SIGNING_PRIVATE_KEY: "UPDATE_FEED_SIGNING_KEY",
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "UPDATE_FEED_SIGNING_KEY_PASSWORD",
};
// how the sign step runs the signer: by its path under RUNNER_TEMP, without a
// network, bound to the release's version.
const SIGNER_CALL =
  '"$SYSTEM_BIN/unshare" --map-root-user --net "${RUNNER_TEMP}/update-tools/cargo-tauri" signer sign --app-version "$VERSION"';
const SIGNING_ACTIONS = [
  "actions/download-artifact",
  "actions/upload-artifact",
];
// the `with:` keys the artifact actions may take in sign-update. any other, such
// as `repository`, `run-id` or `github-token`, could point them elsewhere.
const ARTIFACT_INPUTS = {
  "actions/download-artifact": ["name", "path"],
  "actions/upload-artifact": [
    "name",
    "path",
    "retention-days",
    "if-no-files-found",
  ],
};
const BIN_TOOLS = ["awk", "sha256sum", "unshare"];
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
// shell built-ins and base utilities, which sign-update may call by name.
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
  "mktemp",
  "find",
  "sort",
  "tr",
  "wc",
  "head",
  "tail",
  "cut",
  "true",
  "false",
  "exit",
  "return",
  "local",
  "read",
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
// the jobs of release.yaml after plan and build, with what each needs directly.
const CHAIN = {
  "update-tool": ["plan"],
  "sign-update": ["plan", "build", "update-tool"],
  "assemble-signed": ["plan", "build", "update-tool", "sign-update"],
  attest: ["plan", "assemble-signed"],
  release: ["plan", "attest"],
};
const LIMITS = { "update-tool": 40, "sign-update": 15, "assemble-signed": 10 };
// the jobs that run every step they have: a step condition could skip a check.
const CONDITION_FREE_JOBS = ["update-tool", "sign-update", "assemble-signed"];
// GitHub reads the names of its status functions without regard to case, so
// `Always()` and `always ( )` are the function too.
const FAILURE_FUNCTION = /\b(always|failure|cancelled)\s*\(\s*\)/i;

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
const scriptsOf = (job) =>
  stepsOf(job).filter((step) => typeof step.run === "string");
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
const isSigningStep = (step) =>
  typeof step.run === "string" && step.run.split("\n")[0] === SIGNING_STEP;

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

// ---- the secrets ---------------------------------------------------------

// in release.yaml and release-build.yaml, any read of the `secrets` context,
// by any name, outside sign-update's sign step, and there only the two
// variables of SIGNING_SECRETS. in every other workflow file, a read of a
// signing secret by name, of the whole context, or `secrets: inherit`. this
// reads the parsed documents, not the scripts' text.
const ANY_SECRET = /\bsecrets\s*[.[]|\(\s*secrets\s*\)/i;
const SIGNING_SECRET =
  /\bsecrets\s*(?:\.\s*(?:MACOS_|UPDATE_FEED_)|\[)|\(\s*secrets\s*\)/i;
const RELEASE_FILES = ["release.yaml", "release-build.yaml"];
function secretReads(value, pattern, path = "") {
  if (typeof value === "string") {
    return pattern.test(value) ? [`${path} reads ${value.trim()}`] : [];
  }
  if (Array.isArray(value)) {
    return value.flatMap((item, index) =>
      secretReads(item, pattern, `${path}[${index}]`),
    );
  }
  if (value && typeof value === "object") {
    return Object.entries(value).flatMap(([key, item]) => {
      const here = path === "" ? key : `${path}.${key}`;
      return key === "secrets" && item === "inherit"
        ? [`${here} passes every secret on`]
        : secretReads(item, pattern, here);
    });
  }
  return [];
}
// the document without the sign step's two signing variables, which the
// signing job's own check reads.
function withoutSigningVariables(doc) {
  const job = doc.jobs?.[SIGNING_JOB];
  if (!job) return doc;
  const steps = stepsOf(job).map((step) =>
    isSigningStep(step)
      ? {
          ...step,
          env: Object.fromEntries(
            Object.entries(step.env ?? {}).filter(
              ([name]) => !(name in SIGNING_SECRETS),
            ),
          ),
        }
      : step,
  );
  return { ...doc, jobs: { ...doc.jobs, [SIGNING_JOB]: { ...job, steps } } };
}
for (const [file, doc] of Object.entries(workflows)) {
  const releaseFile = RELEASE_FILES.includes(file);
  const read = file === "release.yaml" ? withoutSigningVariables(doc) : doc;
  for (const use of secretReads(
    read,
    releaseFile ? ANY_SECRET : SIGNING_SECRET,
  )) {
    problem(
      releaseFile
        ? `${file}: ${use}; only ${SIGNING_JOB}'s sign step may read a secret in the release workflows`
        : `${file}: ${use}; only release.yaml's ${SIGNING_JOB} may read a signing secret`,
    );
  }
}

// ---- sign-update ---------------------------------------------------------

const jobs = release ? (release.jobs ?? {}) : {};
const signing = jobs[SIGNING_JOB];
// a script's lines without comments or line continuations.
function codeLines(script) {
  return script
    .replace(/\\\n/g, " ")
    .split("\n")
    .filter((line) => !line.trimStart().startsWith("#"));
}

// the commands that assign to a variable given by name, and `for`, which does the
// same, when they start a command: `read PATH`, `export PATH`, `local ENV`. the
// plain `NAME=` forms are matched separately.
const COMMAND_START =
  "(?:^|[;&|({]|\\b(?:then|do|else)\\b)[ \\t]*(?:[A-Za-z_]\\w*=\\S*[ \\t]+)*(?:(?:command|builtin)[ \\t]+)?";
const PROTECTED_NAME =
  "(?<![A-Za-z0-9_$\\{])[\"']?(PATH|BASH_ENV|ENV)[\"']?(?![A-Za-z0-9_])";
const NAMING_COMMAND = new RegExp(
  `${COMMAND_START}(read|mapfile|readarray|getopts|declare|typeset|local|export|readonly|for|select)\\b[^;&|\\n]*?${PROTECTED_NAME}`,
  "gm",
);
const PRINTF_TO_VARIABLE = new RegExp(
  `${COMMAND_START}printf\\b[^;&|\\n]*?[ \\t]-v[ \\t]*${PROTECTED_NAME}`,
  "gm",
);

const TOOL_PATH = /^\$\{?SYSTEM_S?BIN\}?\/[A-Za-z0-9_]+$/;
// the character that stands for one character of a blanked string. no command
// or option is spelled with it.
const BLANK = "\u0001";
// a quoted string that starts with `-` could be an option, so it is kept.
const OPTION_LIKE = /^-[^\s"'`()\\]*$/;

// the script with the text of every quoted string blanked, so that a `;` or a
// tool's name inside a message is not read as a command. a `$(...)` inside a
// string is kept, and so is a string that is only a tool's path or that could
// be an option, which find's arguments are checked for.
function skeleton(script) {
  const source = script.replace(/\\\n/g, " ");
  function scan(start, closer) {
    let out = "";
    let i = start;
    while (i < source.length) {
      const char = source[i];
      if (closer && char === ")") return [out, i + 1];
      if (char === "`") {
        throw new Error("a backtick command substitution");
      } else if (char === "\\") {
        out += BLANK + BLANK;
        i += 2;
      } else if (char === "'") {
        const end = source.indexOf("'", i + 1);
        if (end === -1) throw new Error("an unterminated single quote");
        const raw = source.slice(i + 1, end);
        out += `'${OPTION_LIKE.test(raw) ? raw : BLANK.repeat(end - i - 1)}'`;
        i = end + 1;
      } else if (char === "$" && source[i + 1] === "'") {
        let j = i + 2;
        while (j < source.length && source[j] !== "'")
          j += source[j] === "\\" ? 2 : 1;
        const raw = source.slice(i + 2, j);
        out += `$'${OPTION_LIKE.test(raw) ? raw : BLANK.repeat(j - i - 2)}'`;
        i = j + 1;
      } else if (char === '"') {
        let j = i + 1;
        let inner = "";
        while (j < source.length && source[j] !== '"') {
          if (source[j] === "`") {
            throw new Error("a backtick command substitution");
          } else if (source[j] === "\\") {
            inner += BLANK + BLANK;
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
            inner += BLANK;
            j += 1;
          }
        }
        if (j >= source.length) throw new Error("an unterminated double quote");
        const raw = source.slice(i + 1, j);
        out += `"${TOOL_PATH.test(raw) || OPTION_LIKE.test(raw) ? raw : inner}"`;
        i = j + 1;
      } else if (source.startsWith("$((", i)) {
        const end = source.indexOf("))", i);
        if (end === -1) throw new Error("an unterminated arithmetic expansion");
        // the expansion is replaced by `0` below, which would hide a command.
        if (/\$\(|`/.test(source.slice(i + 3, end))) {
          throw new Error("an arithmetic expansion that contains a command");
        }
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

// the options of find that the job may use. -exec, -execdir, -ok and -okdir
// run a command, and a quoted option name is read as the option it spells.
const FIND_OPTIONS = new Set([
  "-mindepth",
  "-maxdepth",
  "-type",
  "-name",
  "-print",
  "-print0",
]);
const TRAP_OPERANDS = new Set(["-", "-p", "-l"]);
const quotedWholly = /^(["'])[^"']*\1$/;
const openingRemnant = new RegExp(`^["']${BLANK}*$`);
const closingRemnant = new RegExp(`^${BLANK}*["']$`);

// the bodies of the `trap '...' SIGNAL` commands of a script, as commands to
// read, and the operands of a trap that are neither a quoted body nor `-`.
function trapOperands(script) {
  const code = script
    .replace(/\\\n/g, " ")
    .split("\n")
    .filter((line) => !line.trimStart().startsWith("#"))
    .join("\n");
  const pattern =
    /(?:^|[;&|({]|\b(?:then|do|else)\b)[ \t]*trap[ \t]+(?:--[ \t]+)?(?:'([^']*)'|"((?:[^"\\]|\\.)*)"|([^\s;&|()'"]+))/gm;
  const bodies = [];
  const bare = [];
  for (const match of code.matchAll(pattern)) {
    if (match[3] !== undefined) bare.push(match[3]);
    else bodies.push(match[1] ?? match[2]);
  }
  return { bodies, bare };
}

// the outermost `$(...)` of a text, each to its closing parenthesis, or to the
// end of the text when it has none.
function substitutionsOf(span) {
  const found = [];
  for (let start = span.indexOf("$("); start !== -1;) {
    let depth = 0;
    let end = start + 1;
    for (; end < span.length; end += 1) {
      if (span[end] === "(") depth += 1;
      else if (span[end] === ")" && --depth === 0) break;
    }
    found.push(span.slice(start, end + 1));
    start = span.indexOf("$(", end + 1);
  }
  return found;
}

// every word a script uses as a command, the functions it defines, and the
// commands it uses in a way this check will not read (`refused`).
function commandWords(script) {
  let flat = skeleton(script);
  const functions = new Set();
  const refused = [];
  // a `[[ ]]` test is not read as commands, but the shell runs the command
  // substitutions it holds, so they stay, as arguments of the test.
  flat = flat.replace(
    /\[\[ .*? \]\]/g,
    (span) => `[[ ]] ${substitutionsOf(span).join(" ")} `,
  );
  flat = flat.replace(/^[ \t]*([A-Za-z_]\w*)\(\)[ \t]*\{/gm, (_, name) => {
    functions.add(name);
    return "";
  });
  flat = flat
    .replace(/[0-9]*[<>]+&[0-9-]+/g, " ")
    .replace(/[0-9]*(?:<<<|<<|>>|>|<)/g, " ");
  const words = [];
  const parts = flat.split(/(\$\(|[;&|()\n])/);
  for (let part = 0; part < parts.length; part += 2) {
    const terminator = parts[part + 1];
    const tokens = parts[part].trim().split(/\s+/).filter(Boolean);
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
    // a substitution that stands where a command word would, after the start of
    // a line, `;`, `&`, `|`, `(`, `{`, a keyword or the assignments before a
    // command, runs whatever it prints. one attached to a word, as in `x=$(...)`,
    // or to the end of a substitution, as in `$(...)$(...)`, does not.
    if (
      word === undefined &&
      terminator === "$(" &&
      !/\S$/.test(parts[part]) &&
      !(parts[part] === "" && parts[part - 1] === ")")
    ) {
      refused.push("runs a command substitution as a command");
    }
    if (word === undefined || word === "for" || word === "}") continue;
    if (["fi", "done", "esac", "in"].includes(word)) continue;
    if (quotedWholly.test(word)) {
      // a quoted word is a command only as a tool's path.
      if (TOOL_PATH.test(word.slice(1, -1))) words.push(word);
      else refused.push("runs a quoted or expanded word as a command");
      continue;
    }
    if (openingRemnant.test(word) || closingRemnant.test(word)) {
      // the rest of a string around a `$(...)`. at the start of a command, one
      // that opens a string is a quoted substitution used as the command.
      if (terminator === "$(" && openingRemnant.test(word)) {
        refused.push("runs a quoted command substitution as a command");
      }
      continue;
    }
    words.push(word);
    if (word === "find") {
      for (const token of tokens.slice(index + 1)) {
        const option = token.replace(/["']/g, "");
        if (option.startsWith("-") && !FIND_OPTIONS.has(option)) {
          refused.push(
            `runs find with ${option}; only ${[...FIND_OPTIONS].join(", ")} are allowed`,
          );
        }
      }
    }
  }
  // a trap's body runs as commands later.
  const { bodies, bare } = trapOperands(script);
  for (const body of bodies) {
    const inner = commandWords(body);
    words.push(...inner.words);
    refused.push(...inner.refused);
    for (const name of inner.functions) functions.add(name);
  }
  for (const operand of bare) {
    if (!TRAP_OPERANDS.has(operand) && !functions.has(operand)) {
      refused.push(
        `sets a trap to ${operand}, which is neither a quoted command nor a function of the script`,
      );
    }
  }
  return { words, functions, refused };
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
  if (signing["runs-on"] !== "ubuntu-latest")
    problem(`${where} must run on ubuntu-latest`);
  // a shell other than the default could read a file first or run its own
  // program, and the job's scripts are read as bash.
  for (const [scope, defaults] of [
    ["the workflow", release.defaults],
    [SIGNING_JOB, signing.defaults],
  ]) {
    if (defaults?.run && "shell" in defaults.run) {
      problem(`${where}: ${scope} sets defaults.run.shell`);
    }
  }

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
      } else {
        if (!/@v\d+$/.test(step.uses)) {
          problem(
            `${where}: step "${label}" uses ${step.uses}, not a major tag`,
          );
        }
        for (const key of Object.keys(step.with ?? {})) {
          if (!ARTIFACT_INPUTS[action].includes(key)) {
            problem(
              `${where}: step "${label}" gives ${action} the input ${key}; only ${ARTIFACT_INPUTS[action].join(", ")} are allowed`,
            );
          }
        }
      }
    }
    if ("shell" in step) problem(`${where}: step "${label}" sets shell`);
    if (typeof step.run === "string") {
      if (step.run.includes("${{"))
        problem(`${where}: step "${label}" expands \${{ }} inside run`);
      scripts.push({ label, script: step.run, step });
    }
  }

  const bin = new Set();
  for (const { label, script } of scripts) {
    const first = script.split("\n")[0];
    if (!new RegExp(`^# ${SIGNING_JOB}: [a-z]+$`).test(first)) {
      problem(
        `${where}: the script of step "${label}" does not start with a "# ${SIGNING_JOB}: <name>" marker line`,
      );
    }
    for (const tool of usedTools(script, "SYSTEM_BIN")) bin.add(tool);
    if (/\$\{?SYSTEM_SBIN\b/.test(script)) {
      problem(
        `${where}: the script of step "${label}" uses SYSTEM_SBIN; ${SIGNING_JOB} calls only $SYSTEM_BIN/${BIN_TOOLS.join(", ")}`,
      );
    }

    // the stand-in prefix: a literal default, set by nothing else.
    const lines = codeLines(script).map((line) => line.trim());
    if (
      /\$\{?SYSTEM_BIN\b/.test(script) &&
      !lines.includes(': "${SYSTEM_BIN:=/usr/bin}"')
    ) {
      problem(
        `${where}: the script of step "${label}" uses SYSTEM_BIN without the line : "\${SYSTEM_BIN:=/usr/bin}"`,
      );
    }
    for (const match of script.matchAll(/\$\{SYSTEM_BIN[:=?+-]+([^}]*)\}/g)) {
      if (match[0] !== "${SYSTEM_BIN:=/usr/bin}") {
        problem(
          `${where}: the script of step "${label}" gives SYSTEM_BIN a default other than /usr/bin: ${match[0]}`,
        );
      }
    }
    if (
      /\bSYSTEM_BIN\s*=/.test(script) ||
      /\bexport\s+SYSTEM_BIN\b/.test(script)
    ) {
      problem(`${where}: the script of step "${label}" assigns SYSTEM_BIN`);
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

    // the environment the script's commands run in.
    const code = codeLines(script).join("\n");
    for (const match of code.matchAll(/\bGITHUB_(PATH|ENV|OUTPUT)\b/g)) {
      problem(
        `${where}: the script of step "${label}" refers to GITHUB_${match[1]}, which could change what a later step runs`,
      );
    }
    for (const match of code.matchAll(
      /(?<![A-Za-z0-9_])(PATH|BASH_ENV|ENV)\+?=/g,
    )) {
      problem(`${where}: the script of step "${label}" assigns ${match[1]}`);
    }
    for (const match of code.matchAll(NAMING_COMMAND)) {
      problem(
        `${where}: the script of step "${label}" names ${match[2]} to ${match[1]}, which could assign it`,
      );
    }
    for (const match of code.matchAll(PRINTF_TO_VARIABLE)) {
      problem(
        `${where}: the script of step "${label}" names ${match[1]} to printf -v, which assigns it`,
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
    for (const message of analysis.refused) {
      problem(`${where}: step "${label}" ${message}`);
    }
    for (const word of analysis.words) {
      const bare = word.replaceAll('"', "");
      if (TOOL_PATH.test(bare) && (word === bare || word === `"${bare}"`))
        continue;
      if (analysis.functions.has(bare) || ALLOWED_COMMANDS.has(bare)) continue;
      if (PACKAGE_MANAGERS.includes(bare)) {
        problem(`${where}: step "${label}" runs the package manager ${bare}`);
      } else if (FETCHERS.includes(bare)) {
        problem(
          `${where}: step "${label}" runs ${bare}; the job downloads nothing but its artifacts`,
        );
      } else if (BIN_TOOLS.includes(bare)) {
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
  for (const tool of bin) {
    if (!BIN_TOOLS.includes(tool))
      problem(
        `${where} calls $SYSTEM_BIN/${tool}, which is not one of ${BIN_TOOLS.join(", ")}`,
      );
  }

  // the one step that reads the key, how it reads it, and how it runs the
  // signer; no other step runs the signer.
  const signSteps = stepsOf(signing).filter(isSigningStep);
  if (signSteps.length !== 1) {
    problem(`${where} must have exactly one "${SIGNING_STEP}" step`);
    return;
  }
  const [sign] = signSteps;
  for (const [name, secret] of Object.entries(SIGNING_SECRETS)) {
    if (text(sign.env?.[name]).trim() !== `\${{ secrets.${secret} }}`) {
      problem(
        `${where}: the sign step must set ${name} to \${{ secrets.${secret} }}`,
      );
    }
  }
  if (!sign.run.includes(SIGNER_CALL)) {
    problem(
      `${where}: the sign step does not run the signer as ${SIGNER_CALL}`,
    );
  }
  for (const { label, step } of scripts) {
    if (
      step !== sign &&
      /update-tools\/cargo-tauri"?\s+signer/.test(step.run)
    ) {
      problem(
        `${where}: step "${label}" runs the signer outside the sign step`,
      );
    }
  }
}

// ---- update-tool and assemble-signed --------------------------------------

function checkPlainJob(id, script) {
  const job = jobs[id];
  if (!job) return;
  const where = `release.yaml ${id}`;
  if (job["runs-on"] !== "ubuntu-latest")
    problem(`${where} must run on ubuntu-latest`);
  if (!sameMap(job.permissions, { contents: "read" }))
    problem(`${where} must hold exactly \`contents: read\``);
  for (const step of stepsOf(job)) {
    if (
      text(step.uses).startsWith("actions/checkout@") &&
      step.with?.["persist-credentials"] !== "false"
    ) {
      problem(`${where}: a checkout does not set persist-credentials: false`);
    }
    if (
      text(step.uses).startsWith("jdx/mise-action@") &&
      step.with?.cache !== "false"
    ) {
      problem(`${where}: mise-action must set cache: false`);
    }
    if (text(step.uses).startsWith("actions/cache")) {
      problem(`${where} uses a cache`);
    }
  }
  if (!scriptsOf(job).some((step) => step.run.includes(script))) {
    problem(`${where} does not run ${script}`);
  }
}

function checkFeed() {
  const assemble = jobs["assemble-signed"];
  if (!assemble) return;
  const where = "release.yaml assemble-signed";
  const scripts = scriptsOf(assemble)
    .map((step) => step.run)
    .join("\n");
  if (!scripts.includes(".github/scripts/write-update-feed.sh"))
    problem(`${where} does not write the feed with write-update-feed.sh`);
  if (!/"\$check" dist\/latest\.json /.test(scripts))
    problem(`${where} does not run the feed check on dist/latest.json`);
  if (!scripts.includes(".github/scripts/assemble-release.sh signed"))
    problem(`${where} does not run assemble-release.sh in signed mode`);
  // the throwaway-key option is for the merge checks; the release checks the
  // feed against the committed key.
  if (JSON.stringify(release).includes("--public-key"))
    problem(
      "release.yaml passes --public-key; the release checks the feed against the committed key",
    );
}

// ---- the chain -----------------------------------------------------------

function checkChain() {
  // every job but plan needs plan directly and carries the guard.
  for (const [id, job] of Object.entries(jobs)) {
    if (id === "plan") continue;
    if (!asList(job.needs).includes("plan"))
      problem(`release.yaml ${id} must need plan directly`);
    if (text(job.if).trim() !== GUARD)
      problem(`release.yaml ${id} must carry \`if: ${GUARD}\``);
  }
  for (const [id, wanted] of Object.entries(CHAIN)) {
    if (!jobs[id]) {
      problem(`release.yaml has no ${id} job`);
      continue;
    }
    const have = asList(jobs[id].needs);
    for (const name of wanted) {
      if (!have.includes(name))
        problem(`release.yaml ${id} must need ${name} directly`);
    }
  }
  for (const [id, limit] of Object.entries(LIMITS)) {
    const minutes = Number(jobs[id]?.["timeout-minutes"]);
    if (jobs[id] && !(minutes > 0 && minutes <= limit))
      problem(
        `release.yaml ${id} must set timeout-minutes to at most ${limit}`,
      );
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
    if (FAILURE_FUNCTION.test(text(job.if))) {
      problem(
        `release.yaml ${id} has a job-level condition that can run it after a failure: ${job.if}`,
      );
    }
    for (const step of stepsOf(job)) {
      if ("continue-on-error" in step)
        problem(
          `release.yaml ${id}: step "${text(step.name)}" sets continue-on-error`,
        );
      if (FAILURE_FUNCTION.test(text(step.if))) {
        problem(
          `release.yaml ${id}: step "${text(step.name)}" can run after a failure`,
        );
      } else if (CONDITION_FREE_JOBS.includes(id) && "if" in step) {
        problem(
          `release.yaml ${id}: step "${text(step.name)}" has a condition, which could skip it; ${CONDITION_FREE_JOBS.join(", ")} run every step`,
        );
      }
    }
  }
}

// ---- release-build.yaml's assemble ----------------------------------------

// a command that writes a file named SHA256SUMS: a redirection to it, tee, or cp
// or mv with it as the destination, the last argument. reading it (`cat`,
// `sha256sum -c`) is not a write.
const SUMS_FILE = String.raw`["']?[^\s;&|"']*SHA256SUMS["']?(?![\w.-])`;
const SUMS_WRITES = [
  new RegExp(String.raw`>>?[ \t]*${SUMS_FILE}`),
  new RegExp(String.raw`\btee\b[^;&|\n]*[ \t]${SUMS_FILE}`),
  new RegExp(
    String.raw`\b(?:cp|mv)\b[^;&|\n]*[ \t]${SUMS_FILE}[ \t]*(?:$|[;&|)])`,
    "m",
  ),
];

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
      script.includes(".github/scripts/assemble-release.sh unsigned"),
    )
  ) {
    problem(
      "release-build.yaml assemble does not call assemble-release.sh in unsigned mode",
    );
  }
  if (
    scripts.some((script) =>
      SUMS_WRITES.some((write) => write.test(codeLines(script).join("\n"))),
    )
  ) {
    problem(
      "release-build.yaml assemble writes SHA256SUMS itself; assemble-release.sh is the only writer",
    );
  }
}

// no environment of release.yaml may set where commands are found or a file the
// shell runs first.
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
if (release) {
  for (const path of keyPaths(release, /^(PATH|BASH_ENV|ENV|SYSTEM_BIN)$/)) {
    problem(
      `release.yaml: ${path} sets PATH, BASH_ENV, ENV or SYSTEM_BIN; nothing may change what a release job's shell runs`,
    );
  }
}

if (release && releaseBuild) {
  checkSigningJob();
  checkPlainJob("update-tool", ".github/scripts/build-update-tools.sh");
  checkPlainJob("assemble-signed", ".github/scripts/assemble-release.sh");
  checkFeed();
  checkChain();
  checkBuildAssemble();
}

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
