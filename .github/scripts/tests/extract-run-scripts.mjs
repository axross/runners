// reads the `run:` scripts of one workflow job, so a test can run them outside
// GitHub Actions, in one of three forms:
//
//   node extract-run-scripts.mjs <workflow file> <job id> <output directory>
//   node extract-run-scripts.mjs --step <workflow file> <job id> <step name>
//   node extract-run-scripts.mjs --step-env <workflow file> <job id> <step name> <env name>
//
// the first form writes every script of the job to a file and lists every step in
// steps.json. a script is written as NN-<name>.sh, where <name> comes from the marker
// line `# <job id>: <name>` that must be the script's first line. steps.json holds
// each step's kind (run or uses), name, id, condition and env, and a uses step's
// action and inputs, so the test can run each script with exactly the environment
// its step declares.
//
// --step writes one step's `run:` script to standard output, and --step-env writes the
// value that step declares for one environment variable. the step is the one named
// exactly <step name>, and neither form needs a marker line. a missing job, a missing or
// repeated step name, a step without a `run:` script and an undeclared variable each
// exit with status 1 and a message, so a renamed step cannot yield an empty script.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readWorkflow } from "../workflow-reader.mjs";

const USAGE = `Usage: extract-run-scripts.mjs <workflow file> <job id> <output directory>
       extract-run-scripts.mjs --step <workflow file> <job id> <step name>
       extract-run-scripts.mjs --step-env <workflow file> <job id> <step name> <env name>`;

function usageError() {
  console.error(USAGE);
  process.exit(2);
}

function refuse(message) {
  console.error(message);
  process.exit(1);
}

function stepsOf(workflowFile, jobId) {
  const job = readWorkflow(workflowFile).jobs?.[jobId];
  if (!job || !Array.isArray(job.steps))
    refuse(`${workflowFile} has no job ${jobId} with steps`);
  return job.steps;
}

function namedStep(workflowFile, jobId, stepName) {
  const matches = stepsOf(workflowFile, jobId).filter(
    (step) => step.name === stepName,
  );
  if (matches.length === 0)
    refuse(`job ${jobId} of ${workflowFile} has no step named ${stepName}`);
  if (matches.length > 1)
    refuse(
      `job ${jobId} of ${workflowFile} has ${matches.length} steps named ${stepName}`,
    );
  const [step] = matches;
  if (typeof step.run !== "string")
    refuse(`the step ${stepName} of job ${jobId} has no run script`);
  return step;
}

function writeAllScripts(workflowFile, jobId, outputDirectory) {
  const jobSteps = stepsOf(workflowFile, jobId);
  mkdirSync(outputDirectory, { recursive: true });
  const steps = [];
  let number = 0;
  for (const step of jobSteps) {
    const entry = {
      name: step.name ?? null,
      id: step.id ?? null,
      if: step.if ?? null,
      env: step.env ?? {},
    };
    if (typeof step.run === "string") {
      const marker = step.run
        .split("\n")[0]
        .match(new RegExp(`^# ${jobId}: ([a-z]+)$`));
      if (!marker)
        refuse(
          `the script of step "${step.name}" does not start with a "# ${jobId}: <name>" marker line`,
        );
      number += 1;
      const file = `${String(number).padStart(2, "0")}-${marker[1]}.sh`;
      writeFileSync(join(outputDirectory, file), step.run);
      steps.push({ ...entry, kind: "run", marker: marker[1], script: file });
    } else {
      steps.push({
        ...entry,
        kind: "uses",
        uses: step.uses ?? null,
        with: step.with ?? {},
      });
    }
  }
  writeFileSync(
    join(outputDirectory, "steps.json"),
    `${JSON.stringify(steps, null, 2)}\n`,
  );
}

const [, , first, ...rest] = process.argv;
if (first === "--step") {
  if (rest.length !== 3) usageError();
  process.stdout.write(namedStep(...rest).run);
} else if (first === "--step-env") {
  if (rest.length !== 4) usageError();
  const [workflowFile, jobId, stepName, envName] = rest;
  const value = namedStep(workflowFile, jobId, stepName).env?.[envName];
  if (typeof value !== "string")
    refuse(`the step ${stepName} of job ${jobId} declares no env ${envName}`);
  process.stdout.write(value);
} else {
  if (first?.startsWith("--") || rest.length !== 2) usageError();
  writeAllScripts(first, ...rest);
}
