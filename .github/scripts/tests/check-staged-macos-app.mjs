// the checks of the staged Mac app signing steps that read files rather than run
// scripts, which test-staged-sign-macos.sh runs first:
//
//   node check-staged-macos-app.mjs <repository root>
//
//   - no file under .github/workflows/ names the staged file, so no workflow
//     runs or calls its text;
//   - the staged file names no Installer secret or certificate;
//   - the staged verify-macos runs test-signed-dmg.sh, which
//     test-signed-dmg-refusals.sh tests;
//   - the staged `hdiutil create` arguments equal release-build.yaml's app job's;
//   - every tool the staged jobs and test-signed-dmg.sh call, through SYSTEM_BIN or
//     SYSTEM_SBIN or by a literal /usr/bin or /usr/sbin path, is in the lists of
//     the app job's measured-tools step.
// it prints one `ok` line per check, or every problem and exit status 1.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { readWorkflow } from "../workflow-reader.mjs";

const root = process.argv[2];
if (process.argv.length !== 3) {
  console.error("Usage: check-staged-macos-app.mjs <repository root>");
  process.exit(2);
}

const STAGED = ".github/staged/release-macos-app.yaml";
const problems = [];

const stagedJobs = readWorkflow(join(root, STAGED)).jobs ?? {};
const appJob = readWorkflow(join(root, ".github/workflows/release-build.yaml"))
  .jobs?.["app-macos"];
if (!appJob) problems.push("release-build.yaml has no app-macos job");

const runScripts = (job) =>
  (job?.steps ?? []).flatMap((step) =>
    typeof step.run === "string" ? [{ name: step.name, script: step.run }] : [],
  );
const code = (script) =>
  script
    .split("\n")
    .filter((line) => !/^\s*#/.test(line))
    .join("\n");

// the staged file's text is named by no workflow, whatever the extension.
const workflowsDirectory = join(root, ".github/workflows");
for (const file of readdirSync(workflowsDirectory)) {
  if (
    readFileSync(join(workflowsDirectory, file), "utf8").includes(
      "release-macos-app",
    )
  ) {
    problems.push(`.github/workflows/${file} names the staged file`);
  }
}

// the Installer certificate belongs to the command line's package, which no
// staged step builds.
if (readFileSync(join(root, STAGED), "utf8").includes("INSTALLER")) {
  problems.push(
    "the staged file names the Installer certificate or its secret",
  );
}

if (
  !runScripts(stagedJobs["verify-macos"]).some(({ script }) =>
    script.includes("test-signed-dmg.sh"),
  )
) {
  problems.push("the staged verify-macos does not run test-signed-dmg.sh");
}

// the arguments of the `until <hdiutil> create ...; do` line of a script, or null
// when the script has none or more than one.
function hdiutilCreate(script) {
  const matches = [
    ...code(script).matchAll(
      /^\s*until\s+"?[^\s"]*hdiutil"?\s+create\s+([^;\n]*);\s*do\s*$/gm,
    ),
  ];
  return matches.length === 1 ? matches[0][1].trim() : null;
}
const stagedImaging = Object.values(stagedJobs)
  .flatMap(runScripts)
  .map(({ script }) => hdiutilCreate(script))
  .filter(Boolean);
const builtImaging = runScripts(appJob)
  .map(({ script }) => hdiutilCreate(script))
  .filter(Boolean);
if (stagedImaging.length !== 1) {
  problems.push(
    `the staged jobs call hdiutil create ${stagedImaging.length} times, not once`,
  );
} else if (builtImaging.length !== 1) {
  problems.push("the app-macos job does not call hdiutil create exactly once");
} else if (stagedImaging[0] !== builtImaging[0]) {
  problems.push(
    `hdiutil create's arguments differ from the app job's:\n    ${stagedImaging[0]}\n    ${builtImaging[0]}`,
  );
}

// the tools the app job measures: its two lists and the xcrun tools it looks up.
const measuring = runScripts(appJob).find(
  ({ name }) => name === "Check The Signing Tools",
);
const measured = new Set();
if (!measuring) {
  problems.push("the app-macos job has no Check The Signing Tools step");
} else {
  for (const list of ["bin_tools", "sbin_tools"]) {
    const found = measuring.script.match(
      new RegExp(`^\\s*${list}="([^"]*)"`, "m"),
    );
    if (!found) problems.push(`the measuring step has no ${list} list`);
    for (const tool of (found?.[1] ?? "").split(/\s+/).filter(Boolean)) {
      measured.add(
        `${list === "bin_tools" ? "/usr/bin" : "/usr/sbin"}/${tool}`,
      );
    }
  }
  const xcrun = measuring.script.match(/^\s*for tool in ([a-z ]+); do\s*$/m);
  for (const tool of (xcrun?.[1] ?? "").split(/\s+/).filter(Boolean)) {
    measured.add(`xcrun ${tool}`);
  }
}

// every tool a script calls through its prefix variables or by a literal path, and
// the xcrun tools.
const scripts = [
  ...Object.values(stagedJobs)
    .flatMap(runScripts)
    .map(({ script }) => script),
  readFileSync(join(root, ".github/scripts/tests/test-signed-dmg.sh"), "utf8"),
];
const called = new Set();
for (const script of scripts) {
  for (const [, prefix, tool] of code(script).matchAll(
    /"\$SYSTEM_(S?BIN)\/([a-z]+)"/g,
  )) {
    called.add(`${prefix === "BIN" ? "/usr/bin" : "/usr/sbin"}/${tool}`);
  }
  // a tool called by its literal path, as verify-macos's hash step calls shasum.
  for (const [, directory, tool] of code(script).matchAll(
    /(?<![\w/$])\/usr\/(s?bin)\/([a-z]+)\b/g,
  )) {
    called.add(`/usr/${directory}/${tool}`);
  }
  for (const [, tool] of code(script).matchAll(
    /"\$SYSTEM_BIN\/xcrun" (notarytool|stapler)\b/g,
  )) {
    called.add(`xcrun ${tool}`);
  }
}
if (called.size === 0) problems.push("the staged scripts call no tool");
for (const tool of [...called].sort()) {
  if (!measured.has(tool)) {
    problems.push(
      `the staged scripts call ${tool}, which the app job does not measure`,
    );
  }
}

if (problems.length > 0) {
  for (const message of problems) console.error(`error: ${message}`);
  process.exit(1);
}
console.log("ok   no workflow names the staged file");
console.log("ok   the staged file names no Installer secret");
console.log("ok   the staged verify-macos runs test-signed-dmg.sh");
console.log("ok   the staged hdiutil create arguments equal the app job's");
console.log(
  `ok   the app job measures all ${called.size} tools the staged scripts call`,
);
