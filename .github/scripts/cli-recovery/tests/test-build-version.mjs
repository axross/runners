import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { readWorkflow } from "../../workflow-reader.mjs";

const workflow = readWorkflow(
  new URL("../../../workflows/cli-recovery-build.yaml", import.meta.url),
);
const versionCheck = workflow.jobs.build.steps.find(
  (step) => step.name === "Check The Binary's Version",
).run;

for (const [printed, exitCode, accepted] of [
  ["marshal 1.2.3", 0, true],
  ["marshal 1.2.3\r", 0, true],
  ["marshal 1.2.4", 0, false],
  ["marshal-app 1.2.3", 0, false],
  ["instance-helper 1.2.3", 0, false],
  ["", 0, false],
  ["marshal 1.2.3", 1, false],
]) {
  test(`the actual workflow version check: ${JSON.stringify(printed)}, exit ${exitCode}`, () => {
    const work = mkdtempSync(join(tmpdir(), "test-cli-recovery-version-"));
    try {
      mkdirSync(join(work, "release"));
      writeFileSync(
        join(work, "release", "marshal"),
        `#!/usr/bin/env bash\nprintf '%s\\n' "$PRINTED"\nexit "$EXIT_CODE"\n`,
        { mode: 0o755 },
      );
      const result = spawnSync(
        "bash",
        ["-euo", "pipefail", "-c", versionCheck],
        {
          encoding: "utf8",
          env: {
            ...process.env,
            CARGO_TARGET_DIR: work,
            SUFFIX: "",
            VERSION: "1.2.3",
            PRINTED: printed,
            EXIT_CODE: String(exitCode),
          },
        },
      );
      assert.equal(result.error, undefined);
      assert.equal(
        result.status === 0,
        accepted,
        result.stdout + result.stderr,
      );
      if (accepted)
        assert.match(result.stdout, /The binary prints 'marshal 1\.2\.3'/);
    } finally {
      rmSync(work, { recursive: true, force: true });
    }
  });
}
