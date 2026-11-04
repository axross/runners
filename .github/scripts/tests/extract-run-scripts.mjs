// writes the `run:` scripts of one workflow job to files, so a test can run them
// outside GitHub Actions, and lists every step of the job in steps.json:
//
//   node extract-run-scripts.mjs <workflow file> <job id> <output directory>
//
// a script is written as NN-<name>.sh, where <name> comes from the marker line
// `# <job id>: <name>` that must be the script's first line. steps.json holds
// each step's kind (run or uses), name, id, condition and env, and a uses step's
// action and inputs, so the test can run each script with exactly the environment
// its step declares.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readWorkflow } from "../workflow-reader.mjs";

if (process.argv.length !== 5) {
  console.error(
    "Usage: extract-run-scripts.mjs <workflow file> <job id> <output directory>",
  );
  process.exit(2);
}
const [, , workflowFile, jobId, outputDirectory] = process.argv;

const workflow = readWorkflow(workflowFile);
const job = workflow.jobs?.[jobId];
if (!job || !Array.isArray(job.steps)) {
  console.error(`${workflowFile} has no job ${jobId} with steps`);
  process.exit(1);
}

mkdirSync(outputDirectory, { recursive: true });
const steps = [];
let number = 0;
for (const step of job.steps) {
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
    if (!marker) {
      console.error(
        `the script of step "${step.name}" does not start with a "# ${jobId}: <name>" marker line`,
      );
      process.exit(1);
    }
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
