// reads one step of a workflow job, so a test can run the step's `run:` script outside
// GitHub Actions, in one of two forms:
//
//   node extract-run-scripts.mjs --step <workflow file> <job id> <step name>
//   node extract-run-scripts.mjs --step-env <workflow file> <job id> <step name> <env name>
//
// --step writes the step's `run:` script to standard output, and --step-env writes the
// value that step declares for one environment variable. the step is the one named
// exactly <step name>. a missing job, a missing or repeated step name, a step without a
// `run:` script and an undeclared variable each exit with status 1 and a message, so a
// renamed step cannot yield an empty script. any other arguments exit with status 2 and
// the usage.
import { readWorkflow } from "../workflow-reader.mjs";

const USAGE = `Usage: extract-run-scripts.mjs --step <workflow file> <job id> <step name>
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
  usageError();
}
