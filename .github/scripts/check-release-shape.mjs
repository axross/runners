// fails unless the release workflows keep the shape docs/security/release-authority.md
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
// dispatch. it reads the commands a signing script runs, a trap's body
// included, but not what its commands are told to do, so these can still run
// code it does not see:
//   - the programs of awk and sed, and the operands of a `[[ ]]` test, of which
//     it reads only the command substitutions;
//   - an option or variable name built at run time, such as `find "$option"` or
//     `read -r "$name"` and `printf -v "$name"`;
//   - an assignment through `${PATH:=...}`, and any environment variable other
//     than PATH, BASH_ENV and ENV.
// it also does not see a write to SHA256SUMS in either build's assemble
// job other than by a redirection, tee, cp or mv. a later signing job or step
// (W4, M2, M4) is added to this check in its own pull request, under REVIEW.md's
// release lens.
import { readdirSync, readFileSync } from "node:fs";
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
const HISTORICAL_SOURCE = "dc8e1ebcc0722997206487f3dcca2ae7e519cf02";
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
// the identities step hands the two names the later steps need on through
// GITHUB_ENV, in one block of `echo "NAME=${variable}"` lines.
const EXPORTED = ["APPLICATION_IDENTITY", "INSTALLER_IDENTITY_NAME"];
const EXPORT_BLOCK = new RegExp(
  `\\{\\n(?:[ \\t]*echo "(?:${EXPORTED.join("|")})=\\$\\{[a-z_]+\\}"\\n)+[ \\t]*\\} >>"\\$GITHUB_ENV"`,
);
const GUARD_CONTEXT = "every new job";
// the jobs that run every step they have: a step condition could skip a check.
const CONDITION_FREE_JOBS = [
  "verify-macos",
  "assemble-signed",
  "attest",
  "release",
];
// the runner labels of the jobs after sign-macos, all GitHub-hosted. sign-macos
// has its own rule.
const RUNNERS = {
  "verify-macos": "macos-latest",
  "assemble-signed": "ubuntu-latest",
};
const LIMITS = { "sign-macos": 60, "verify-macos": 20, "assemble-signed": 10 };
// GitHub reads the names of its status functions without regard to case, so
// `Always()` and `always ( )` are the function too.
const FAILURE_FUNCTION = /\b(always|failure|cancelled)\s*\(\s*\)/i;
// the `with:` keys the artifact actions may take in sign-macos. any other, such
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
const recoveryBuild = workflows["cli-recovery-build.yaml"];
if (!release) problem("release.yaml is missing or unreadable");
if (!releaseBuild) problem("release-build.yaml is missing or unreadable");
if (!recoveryBuild) problem("cli-recovery-build.yaml is missing or unreadable");

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

/** detects an Environment declared on a job or a misplaced step. */
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

// ---- the signing secrets -------------------------------------------------

// only sign-macos reads a signing secret, whether by name, through the whole
// `secrets` context, or by passing all of them (`secrets: inherit`) to a called
// workflow. every workflow file is read, and none but release.yaml's sign-macos
// is exempt. this reads the parsed documents, not the scripts' text.
const SECRET_READ = /\bsecrets\s*(?:\.\s*APPLE_|\[)|\(\s*secrets\s*\)/i;
function secretReads(value, path = "") {
  if (typeof value === "string") {
    return SECRET_READ.test(value) ? [`${path} reads ${value.trim()}`] : [];
  }
  if (Array.isArray(value)) {
    return value.flatMap((item, index) =>
      secretReads(item, `${path}[${index}]`),
    );
  }
  if (value && typeof value === "object") {
    return Object.entries(value).flatMap(([key, item]) => {
      const here = path === "" ? key : `${path}.${key}`;
      return key === "secrets" && item === "inherit"
        ? [`${here} passes every secret on`]
        : secretReads(item, here);
    });
  }
  return [];
}
for (const [file, doc] of Object.entries(workflows)) {
  const rest = { ...doc, jobs: { ...doc.jobs } };
  if (file === "release.yaml") delete rest.jobs[SIGNING_JOB];
  for (const use of secretReads(rest)) {
    problem(`${file}: ${use}; only ${SIGNING_JOB} may read a signing secret`);
  }
}

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

/**
 * blanks quoted text without hiding command substitutions, tool paths or
 * option-like strings from the command checks.
 * @throws {Error} for backticks, unterminated quotes or arithmetic, or a
 * command inside arithmetic that blanking would conceal.
 */
function skeleton(script) {
  const source = script.replace(/\\\n/g, " ");
  /**
   * returns a blanked span and the offset after its closing parenthesis, or
   * the end of source for the outer span.
   * @throws {Error} for backticks, unterminated quotes or arithmetic, or a
   * command inside arithmetic that blanking would conceal.
   */
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

/**
 * returns command words, local function names and refused command forms,
 * including commands held in traps and substitutions.
 * @throws {Error} when skeleton cannot safely blank the script or a trap body.
 */
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

/** lists tools referenced through a system-tool prefix, excluding comments. */
function usedTools(script, variable) {
  const found = new Set();
  const pattern = new RegExp(`\\$\\{?${variable}\\}?/([A-Za-z0-9_]+)`, "g");
  for (const line of codeLines(script)) {
    for (const match of line.matchAll(pattern)) found.add(match[1]);
  }
  return found;
}

/**
 * records a signing script's command and environment violations in problems;
 * unsupported shell syntax is reported without executing it.
 */
function checkSigningScript(where, label, script) {
  const first = script.split("\n")[0];
  if (!/^# sign-macos: [a-z]+$/.test(first)) {
    problem(
      `${where}: the script of step "${label}" does not start with a "# sign-macos: <name>" marker line`,
    );
  }
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

  // the environment the script's commands run in.
  const code = codeLines(script).join("\n");
  if (/\bGITHUB_PATH\b/.test(code)) {
    problem(
      `${where}: the script of step "${label}" refers to GITHUB_PATH, which could change where the tools are found`,
    );
  }
  if (EXPORT_BLOCK.test(code)) {
    // the one block that sets the two names the later steps read.
    if (code.replace(EXPORT_BLOCK, "").includes("GITHUB_ENV"))
      problem(
        `${where}: the script of step "${label}" writes GITHUB_ENV outside its block of ${EXPORTED.join(" and ")}`,
      );
  } else if (/\bGITHUB_ENV\b/.test(code)) {
    problem(
      `${where}: the script of step "${label}" refers to GITHUB_ENV other than to set ${EXPORTED.join(" and ")}`,
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

  let analysis;
  try {
    analysis = commandWords(script);
  } catch (error) {
    problem(
      `${where}: the script of step "${label}" cannot be analysed: ${error.message}`,
    );
    return;
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

/**
 * records signing-job privilege, artifact, cleanup and tool-inventory
 * violations in problems; individual script boundaries are checked separately.
 */
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
      for (const [name, value] of Object.entries(step.env ?? {})) {
        if (!name.startsWith("APPLE_")) continue;
        const context = name === "APPLE_TEAM_ID" ? "vars" : "secrets";
        if (value !== `\${{ ${context}.${name} }}`) {
          problem(
            `${where}: step "${label}" must read ${name} from ${context}.${name}`,
          );
        }
      }
      scripts.push({ label, script: step.run, step });
    }
  }

  const bin = new Set();
  const sbin = new Set();
  for (const { label, script } of scripts) {
    checkSigningScript(where, label, script);
    for (const tool of usedTools(script, "SYSTEM_BIN")) bin.add(tool);
    for (const tool of usedTools(script, "SYSTEM_SBIN")) sbin.add(tool);
  }
  // the only steps that carry a condition. any other could skip a signing step
  // (`if: false`) or let one run after a failure.
  for (const step of stepsOf(signing)) {
    if (!("if" in step)) continue;
    const condition = text(step.if);
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

/**
 * records signed-package layout and argument differences from the immutable
 * historical builder in problems; only signing options may differ.
 */
function checkPackageArguments(where, scripts) {
  // the builder lives only in the pinned historical checkout. these are its
  // package contract, not a second executable copy of that builder.
  const identifier = "app.axross.marshal";
  for (const { label, script } of scripts) {
    for (const match of script.matchAll(/^\s*identifier="([^"]*)"\s*$/gm)) {
      if (match[1] !== identifier)
        problem(
          `${where}: step "${label}" sets identifier "${match[1]}", not build-pkg.sh's "${identifier}"`,
        );
    }
  }
  const layout = [
    'mkdir -p "${stage}/usr/local/bin"',
    'cp "$binary" "${stage}/usr/local/bin/marshal"',
    'chmod 0755 "$stage" "${stage}/usr" "${stage}/usr/local" "${stage}/usr/local/bin" "${stage}/usr/local/bin/marshal"',
  ];
  const pkgbuildArguments = [
    "--root",
    '"$stage"',
    "--identifier",
    '"$identifier"',
    "--version",
    '"$version"',
    "--install-location",
    "/",
    "--ownership",
    "recommended",
    '"$output"',
  ];
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

// binds the unsigned package contract to the complete immutable historical tree;
// current app packaging is not an alternative source for it.
function checkHistoricalBuild() {
  for (const id of ["dependencies", "build"]) {
    const job = recoveryBuild.jobs?.[id];
    const steps = stepsOf(job);
    const checkouts = steps.filter(
      (step) =>
        text(step.uses).startsWith("actions/checkout@") &&
        step.with?.path === "historical",
    );
    if (
      checkouts.length !== 1 ||
      checkouts[0].with?.repository !== "axross/runners" ||
      checkouts[0].with?.ref !== HISTORICAL_SOURCE ||
      checkouts[0].with?.["persist-credentials"] !== "false"
    ) {
      problem(
        `cli-recovery-build.yaml ${id} must check out the complete pinned historical source without credentials`,
      );
    }
    const bindings = scriptsOf(job).filter((step) =>
      step.run.includes(".github/scripts/cli-recovery/prepare-source.sh"),
    );
    if (
      "if" in (job ?? {}) ||
      "continue-on-error" in (job ?? {}) ||
      bindings.some((step) => "if" in step || "continue-on-error" in step)
    ) {
      problem(
        `cli-recovery-build.yaml ${id} historical source binding must not be skippable`,
      );
    }
    for (const step of bindings) {
      if (
        step["working-directory"] !== "." ||
        (step.shell ??
          job?.defaults?.run?.shell ??
          recoveryBuild.defaults?.run?.shell) !== "bash" ||
        step.env?.VERSION !== "${{ inputs.version }}"
      ) {
        problem(
          `cli-recovery-build.yaml ${id} historical source binding must use bash at the workflow root with the input version`,
        );
      }
    }
    const expectedPhases =
      id === "dependencies"
        ? ["prepare", "verify"]
        : ["prepare", "verify", "verify"];
    const phases = bindings.map((step) => {
      const lines = codeLines(step.run)
        .map((line) => line.trim())
        .filter(Boolean);
      if (
        lines[0] === 'echo "Current workflow/tag commit: ${WORKFLOW_COMMIT}"'
      ) {
        lines.shift();
      }
      return ["prepare", "verify"].find(
        (phase) =>
          lines.length === 1 &&
          lines[0] ===
            `bash .github/scripts/cli-recovery/prepare-source.sh ${phase} historical "$VERSION"`,
      );
    });
    for (const phase of ["prepare", "verify"]) {
      const count = expectedPhases.filter((value) => value === phase).length;
      if (phases.filter((value) => value === phase).length !== count) {
        problem(
          `cli-recovery-build.yaml ${id} must ${phase} the historical source binding in ${count} distinct unconditional step(s)`,
        );
      }
    }
    if (phases.join() !== expectedPhases.join()) {
      problem(
        `cli-recovery-build.yaml ${id} historical source checks must follow ${expectedPhases.join(" -> ")} in distinct steps`,
      );
    } else {
      const checkpoints = bindings.map((step) => steps.indexOf(step));
      if (
        checkouts.length === 1 &&
        steps.indexOf(checkouts[0]) >= checkpoints[0]
      ) {
        problem(
          `cli-recovery-build.yaml ${id} must prepare the historical source binding after its checkout`,
        );
      }
      const stages =
        id === "dependencies"
          ? [
              [
                "rustup show active-toolchain",
                "jdx/mise-action@",
                "cargo fetch --locked",
                "mise exec cargo-deny -- cargo deny",
              ],
            ]
          : [
              [
                "rustup show active-toolchain",
                "installer-version.sh",
                "cargo test --locked --release",
                "cargo build --locked --release --bin marshal",
              ],
              [
                "fetch-wix.sh",
                "build-msi.ps1",
                "build-pkg.sh",
                "test-msi-installer.ps1",
                "test-pkg-installer.sh",
              ],
              ['hash_file "$ASSET"', 'hash_file "$INSTALLER"'],
            ];
      for (const [stage, commands] of stages.entries()) {
        for (const command of commands) {
          const operations = steps.filter(
            (step) =>
              text(step.run).includes(command) ||
              text(step.uses).startsWith(command),
          );
          const at = steps.indexOf(operations[0]);
          if (
            operations.length !== 1 ||
            at <= checkpoints[stage] ||
            at >= (checkpoints[stage + 1] ?? steps.length)
          ) {
            problem(
              `cli-recovery-build.yaml ${id} historical source checks must surround ${command} in order`,
            );
          }
        }
      }
    }
    const scripts = scriptsOf(job)
      .map((step) => step.run)
      .join("\n");
    if (
      id === "dependencies" &&
      !scripts
        .split("\n")
        .some(
          (line) =>
            line.trim() ===
            "mise exec cargo-deny -- cargo deny --locked check bans licenses sources",
        )
    ) {
      problem(
        "cli-recovery-build.yaml must select only cargo-deny for historical dependency checks",
      );
    }
  }
  const build = recoveryBuild.jobs?.build;
  const packaging = scriptsOf(build).filter((step) =>
    step.run.includes("build-pkg.sh"),
  );
  if (
    build?.defaults?.run?.["working-directory"] !== "historical/cli" ||
    packaging.length !== 1 ||
    packaging[0].run !==
      'bash ../.github/scripts/build-pkg.sh "../../dist/${ASSET}" "$INSTALLER_VERSION" "../../dist/${INSTALLER}"'
  ) {
    problem(
      "cli-recovery-build.yaml must run historical/.github/scripts/build-pkg.sh from historical/cli",
    );
  }
  try {
    if (
      !read(".github/scripts/cli-recovery/prepare-source.sh").includes(
        `source_commit=${HISTORICAL_SOURCE}\n`,
      )
    ) {
      problem("prepare-source.sh must pin the historical source identity");
    }
  } catch {
    problem("cli-recovery/prepare-source.sh is missing");
  }
}

/**
 * records release-chain violations in problems, including missing jobs,
 * privilege changes, unsigned publication inputs and failure bypasses.
 */
function checkNewJobs() {
  const wantedJobs = ["plan", "build", ...NEW_JOBS, "attest", "release"];
  if (Object.keys(jobs).sort().join() !== wantedJobs.sort().join()) {
    problem("release.yaml must contain exactly the seven CLI release jobs");
  }
  if (jobs.build?.uses !== "./.github/workflows/cli-recovery-build.yaml") {
    problem("release.yaml build must call cli-recovery-build.yaml");
  }
  if (!asList(jobs.build?.needs).includes("plan"))
    problem("release.yaml build must need plan directly");
  if (text(jobs.build?.if).trim() !== GUARD)
    problem(`release.yaml build must carry \`if: ${GUARD}\``);
  if (!sameMap(jobs.build?.permissions, { contents: "read" }))
    problem("release.yaml build must hold exactly `contents: read`");
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
      if (job["runs-on"] !== RUNNERS[id])
        problem(`${where} must run on ${RUNNERS[id]}`);
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
  for (const id of ["attest", "release"]) {
    if (jobs[id] && text(jobs[id].if).trim() !== GUARD)
      problem(`release.yaml ${id} must carry \`if: ${GUARD}\``);
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
  const attestations = stepsOf(jobs.attest).filter((step) =>
    text(step.uses).startsWith("actions/attest@"),
  );
  const subjects = ["windows", "windows_msi", "macos", "macos_pkg"].map(
    (name) => `release-files/\${{ steps.files.outputs.${name} }}`,
  );
  if (
    attestations.length !== 1 ||
    text(attestations[0].with?.["subject-path"])
      .trim()
      .split("\n")
      .map((line) => line.trim())
      .sort()
      .join() !== subjects.sort().join()
  ) {
    problem(
      "release.yaml attest must attest exactly the four CLI release subjects",
    );
  }
  if (
    !scriptsOf(jobs["assemble-signed"]).some((step) =>
      step.run.includes(".github/scripts/cli-recovery/assemble-release.sh"),
    )
  ) {
    problem(
      "release.yaml assemble-signed does not call cli-recovery/assemble-release.sh",
    );
  }
  if (
    !scriptsOf(jobs["verify-macos"]).some((step) =>
      step.run.includes(".github/scripts/tests/test-signed-pkg.sh"),
    )
  ) {
    problem("release.yaml verify-macos does not call test-signed-pkg.sh");
  }
  if (
    jobs.release &&
    stepsOf(jobs.release).some((step) =>
      JSON.stringify(step).includes("needs.build"),
    )
  ) {
    problem("release.yaml release must not read the build's artifact");
  }

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
      if (id !== SIGNING_JOB && FAILURE_FUNCTION.test(text(step.if))) {
        problem(
          `release.yaml ${id}: step "${text(step.name)}" can run after a failure`,
        );
      } else if (CONDITION_FREE_JOBS.includes(id) && "if" in step) {
        problem(
          `release.yaml ${id}: step "${text(step.name)}" has a condition, which could skip it; ${CONDITION_FREE_JOBS.join(" and ")} run every step`,
        );
      }
    }
  }
}

/**
 * records missing unprivileged tool measurements in problems; the required set
 * comes from the signing and verification scripts, without executing them.
 */
function checkMeasuredTools() {
  const build = jobsOf(recoveryBuild).find(([id]) => id === "build")?.[1];
  const step = stepsOf(build).find(
    (entry) =>
      typeof entry.run === "string" &&
      entry.run.includes("MEASURED: signing tools"),
  );
  if (!step) {
    problem(
      "cli-recovery-build.yaml's build job has no step that prints the MEASURED: signing tools line",
    );
    return;
  }
  if (step.if !== "runner.os == 'macOS'")
    problem(
      "cli-recovery-build.yaml's signing tools step must run on the macOS leg only",
    );
  if (!/exit 1/.test(step.run))
    problem(
      "cli-recovery-build.yaml's signing tools step does not fail when a tool is missing",
    );
  if (
    "env" in step &&
    Object.keys(step.env ?? {}).some((key) => /SECRET|APPLE_/.test(key))
  ) {
    problem(
      "cli-recovery-build.yaml's signing tools step receives a signing value",
    );
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
        `cli-recovery-build.yaml's signing tools step does not measure /usr/bin/${tool}`,
      );
  for (const tool of needSbin)
    if (!measuredSbin.has(tool))
      problem(
        `cli-recovery-build.yaml's signing tools step does not measure /usr/sbin/${tool}`,
      );
  for (const tool of XCRUN_TOOLS) {
    if (!step.run.includes(`for tool in ${XCRUN_TOOLS.join(" ")}`))
      problem(
        `cli-recovery-build.yaml's signing tools step does not look for ${tool} with xcrun --find`,
      );
  }
}

// ---- release-build.yaml's Mac app job --------------------------------------

const MAC_APP_JOB = "app-macos";
const MAC_APP_ARTIFACT = "marshal-macos-app";
const MAC_APP_ACTIONS = ["actions/checkout", "actions/upload-artifact"];
// the tools the staged Mac signing steps call that the signing tools step must
// measure, whatever else it lists; test-staged-sign-macos.sh compares the rest.
const MAC_APP_MEASURED = ["ditto", "hdiutil"];

/**
 * records a Mac app job that holds a secret, uses an action other than checkout
 * and artifact upload, signs, does not hand over its archive's hash, does not
 * write the release version before it compiles, mixes its files into the unsigned
 * set, or leaves a measured tool out of its signing tools
 * step in problems, without running its scripts.
 */
function checkMacAppJob() {
  const file = "release-build.yaml";
  const job = jobsOf(releaseBuild).find(([id]) => id === MAC_APP_JOB)?.[1];
  if (!job) {
    problem(`${file} has no ${MAC_APP_JOB} job`);
    return;
  }
  if (job["runs-on"] !== "macos-latest")
    problem(`${file}'s ${MAC_APP_JOB} job must run on macos-latest`);
  if (text(job["timeout-minutes"]) !== "45")
    problem(`${file}'s ${MAC_APP_JOB} job must be bounded at 45 minutes`);
  if (text(job.outputs?.app_sha256) !== "${{ steps.hash.outputs.app_sha256 }}")
    problem(
      `${file}'s ${MAC_APP_JOB} job must output app_sha256 from its hash step`,
    );
  if (
    text(releaseBuild?.on?.workflow_call?.outputs?.app_sha256?.value) !==
    `\${{ jobs.${MAC_APP_JOB}.outputs.app_sha256 }}`
  )
    problem(`${file} must hand over the ${MAC_APP_JOB} job's app_sha256`);
  for (const step of stepsOf(job)) {
    const action = text(step.uses).split("@")[0];
    if (action && !MAC_APP_ACTIONS.includes(action))
      problem(
        `${file}'s ${MAC_APP_JOB} job uses ${action}; it may use only ${MAC_APP_ACTIONS.join(" and ")}`,
      );
  }
  const uploads = stepsOf(job).filter((step) =>
    text(step.uses).startsWith("actions/upload-artifact@"),
  );
  if (uploads.length !== 1 || uploads[0].with?.name !== MAC_APP_ARTIFACT)
    problem(
      `${file}'s ${MAC_APP_JOB} job must upload ${MAC_APP_ARTIFACT} once and no other artifact`,
    );
  const build = scriptsOf(job).find((step) =>
    /\bcargo tauri build\b/.test(step.run),
  );
  if (!build || !/--no-sign\b/.test(build.run) || !/--locked\b/.test(build.run))
    problem(
      `${file}'s ${MAC_APP_JOB} job must build with \`cargo tauri build ... --no-sign\` after a --locked install`,
    );
  const scripts = scriptsOf(job);
  const stamp = scripts.findIndex((step) =>
    step.run.includes("set-release-version.sh"),
  );
  const firstCargo = scripts.findIndex((step) => /\bcargo\b/.test(step.run));
  if (stamp === -1 || "if" in scripts[stamp])
    problem(
      `${file}'s ${MAC_APP_JOB} job must write the release version with set-release-version.sh in a step with no condition`,
    );
  else if (firstCargo !== -1 && stamp >= firstCargo)
    problem(
      `${file}'s ${MAC_APP_JOB} job must write the release version before any step that runs cargo`,
    );
  // the measuring step names notarytool and stapler to look them up, so it is
  // left out of the scan for them.
  for (const step of scripts) {
    const measuring = step.run.includes("MEASURED: signing tools");
    const found = [
      /\bcodesign\b[^\n]*\s(?:--sign|-s)\b/,
      /\bsecurity\s+import\b/,
      measuring ? null : /\b(?:notarytool|stapler)\b/,
    ].find((pattern) => pattern?.test(codeLines(step.run).join("\n")));
    if (found)
      problem(
        `${file}'s ${MAC_APP_JOB} job step "${text(step.name)}" signs, imports a certificate, notarizes or staples; the job builds unsigned`,
      );
  }
  const assemble = jobsOf(releaseBuild).find(([id]) => id === "assemble")?.[1];
  if (asList(assemble?.needs).includes(MAC_APP_JOB))
    problem(
      `${file} assemble needs ${MAC_APP_JOB}; the Mac files stay out of the unsigned set`,
    );

  const step = scriptsOf(job).find((entry) =>
    entry.run.includes("MEASURED: signing tools"),
  );
  if (!step) {
    problem(
      `${file}'s ${MAC_APP_JOB} job has no step that prints the MEASURED: signing tools line`,
    );
    return;
  }
  if ("if" in step)
    problem(
      `${file}'s signing tools step has a condition, which could skip it`,
    );
  if (!/exit 1/.test(step.run))
    problem(
      `${file}'s signing tools step does not fail when a tool is missing`,
    );
  if (
    "env" in step &&
    Object.keys(step.env ?? {}).some((key) => /SECRET|APPLE_/.test(key))
  )
    problem(`${file}'s signing tools step receives a signing value`);
  const measured = new Set(
    (step.run.match(/^\s*bin_tools="([^"]*)"/m)?.[1] ?? "")
      .split(/\s+/)
      .filter(Boolean),
  );
  for (const tool of MAC_APP_MEASURED)
    if (!measured.has(tool))
      problem(`${file}'s signing tools step does not measure /usr/bin/${tool}`);
  if (!step.run.includes(`for tool in ${XCRUN_TOOLS.join(" ")}`))
    problem(
      `${file}'s signing tools step does not look for ${XCRUN_TOOLS.join(" and ")} with xcrun --find`,
    );
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

/**
 * records unsafe checkout credentials or checksum writers in problems for
 * either build's unsigned assembly job, without running its scripts.
 */
function checkBuildAssemble(workflow, file, scriptPath) {
  const assemble = jobsOf(workflow).find(([id]) => id === "assemble")?.[1];
  if (!assemble) {
    problem(`${file} has no assemble job`);
    return;
  }
  for (const step of stepsOf(assemble)) {
    if (
      text(step.uses).startsWith("actions/checkout@") &&
      step.with?.["persist-credentials"] !== "false"
    ) {
      problem(
        `${file} assemble: a checkout does not set persist-credentials: false`,
      );
    }
  }
  const scripts = scriptsOf(assemble).map((step) => step.run);
  if (!scripts.some((script) => script.includes(scriptPath))) {
    problem(`${file} assemble does not call assemble-release.sh`);
  }
  if (
    scripts.some((script) =>
      SUMS_WRITES.some((write) => write.test(codeLines(script).join("\n"))),
    )
  ) {
    problem(
      `${file} assemble writes SHA256SUMS itself; assemble-release.sh is the only writer`,
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

// no environment of release.yaml, which sign-macos runs in, may set where
// commands are found or a file the shell runs first.
if (release) {
  for (const path of keyPaths(release, /^(PATH|BASH_ENV|ENV)$/)) {
    problem(
      `release.yaml: ${path} sets PATH, BASH_ENV or ENV; the signing job's tools are called by path and nothing may change what its shell runs`,
    );
  }
}

if (release && releaseBuild && recoveryBuild) {
  checkSigningJob();
  checkHistoricalBuild();
  checkNewJobs();
  checkMeasuredTools();
  checkBuildAssemble(
    recoveryBuild,
    "cli-recovery-build.yaml",
    ".github/scripts/cli-recovery/assemble-release.sh",
  );
  checkBuildAssemble(
    releaseBuild,
    "release-build.yaml",
    ".github/scripts/assemble-release.sh",
  );
  checkMacAppJob();
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
