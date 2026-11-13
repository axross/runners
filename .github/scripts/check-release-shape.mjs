// fails unless the release workflows keep the shape docs/conventions/security.md
// records: that no job declares an Environment or reads a signing secret until
// the next signing job, M2's or W4's, joins them; how the jobs of release.yaml
// chain from plan through assemble-signed to attest and release; and that no job
// can bypass a failed one.
//
//   node .github/scripts/check-release-shape.mjs [root]
//
// `root` is the directory holding `.github/`; it defaults to the repository the
// script sits in, and the test points it at altered copies. the check reads the
// workflows with workflow-reader.mjs and the scripts as text, using only Node.
// it guards against mistakes: a pull request can edit it, so it is no control
// against a hostile one. what keeps pull requests away from the Environment is
// the maintainer's branch rule and the fact that release.yaml runs only on
// dispatch. it does not see a write to SHA256SUMS in release-build.yaml's
// assemble job other than by a redirection, tee, cp or mv, or a step condition in
// attest or release that skips a step, such as `if: false`. a later signing job
// (W4, M2) is added to this check in its own pull request, under REVIEW.md's
// release lens.
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
// the jobs of release.yaml after build, with what each needs directly.
const CHAIN = {
  "assemble-signed": ["plan", "build"],
  attest: ["plan", "assemble-signed"],
  release: ["plan", "attest"],
};
// the jobs that run every step they have: a step condition could skip a check.
const CONDITION_FREE_JOBS = ["assemble-signed"];
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

// a script's lines without comments or line continuations.
function codeLines(script) {
  return script
    .replace(/\\\n/g, " ")
    .split("\n")
    .filter((line) => !line.trimStart().startsWith("#"));
}

// ---- the Environment -----------------------------------------------------

// no job declares one until the next signing job; that job's pull request
// allows it here.
for (const [file, doc] of Object.entries(workflows)) {
  for (const [id, job] of jobsOf(doc)) {
    if ("environment" in job) {
      problem(
        `${file}: job ${id} declares an environment; no job may until the next signing job`,
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

// a workflow that a pull request triggers may call only local workflows, so a
// called workflow that declares an Environment fails the rule above.
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
    if (uses !== "" && !uses.startsWith("./.github/workflows/")) {
      problem(
        `${file}: job ${id} calls ${uses}, which this check cannot follow; a pull request's workflow may call only local workflows`,
      );
    }
  }
}

// ---- the signing secrets -------------------------------------------------

// no job reads a signing secret, whether by name, through the whole `secrets`
// context, or by passing all of them (`secrets: inherit`) to a called workflow.
// every workflow file is read. this reads the parsed documents, not the
// scripts' text.
const SECRET_READ = /\bsecrets\s*(?:\.\s*MACOS_|\[)|\(\s*secrets\s*\)/i;
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
  for (const use of secretReads(doc)) {
    problem(
      `${file}: ${use}; no job may read a signing secret until the next signing job`,
    );
  }
}

// ---- the chain -----------------------------------------------------------

const jobs = release ? (release.jobs ?? {}) : {};

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

  const assemble = jobs["assemble-signed"];
  if (assemble) {
    const where = "release.yaml assemble-signed";
    if (assemble["runs-on"] !== "ubuntu-latest")
      problem(`${where} must run on ubuntu-latest`);
    if (!sameMap(assemble.permissions, { contents: "read" }))
      problem(`${where} must hold exactly \`contents: read\``);
    if (!(
      Number(assemble["timeout-minutes"]) > 0 &&
      Number(assemble["timeout-minutes"]) <= 10
    )) {
      problem(`${where} must set timeout-minutes to at most 10`);
    }
    for (const step of stepsOf(assemble)) {
      if (
        text(step.uses).startsWith("actions/checkout@") &&
        step.with?.["persist-credentials"] !== "false"
      ) {
        problem(`${where}: a checkout does not set persist-credentials: false`);
      }
    }
    if (
      !scriptsOf(assemble).some((step) =>
        step.run.includes(".github/scripts/assemble-release.sh"),
      )
    ) {
      problem(`${where} does not call assemble-release.sh`);
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
          `release.yaml ${id}: step "${text(step.name)}" has a condition, which could skip it; ${CONDITION_FREE_JOBS.join(" and ")} runs every step`,
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
      script.includes(".github/scripts/assemble-release.sh"),
    )
  ) {
    problem("release-build.yaml assemble does not call assemble-release.sh");
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
  for (const path of keyPaths(release, /^(PATH|BASH_ENV|ENV)$/)) {
    problem(
      `release.yaml: ${path} sets PATH, BASH_ENV or ENV; nothing may change what a release job's shell runs`,
    );
  }
}

if (release && releaseBuild) {
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
