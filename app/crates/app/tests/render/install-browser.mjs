// the Linux CI renderer's browser: verified before extraction, confined to the
// worktree. Installing its OS libraries is a separate, CI-only preparation step.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { createWriteStream } from "node:fs";
import { chmod, mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { pipeline } from "node:stream/promises";

assert.equal(process.platform, "linux", "The CI renderer requires Linux x64.");
assert.equal(process.arch, "x64", "The CI renderer requires Linux x64.");
const pin = JSON.parse(
  await readFile(new URL("browser.json", import.meta.url)),
);
const runtime = new URL(
  `../../../../target/render/browser-${pin.version}/`,
  import.meta.url,
);
await mkdir(runtime, { recursive: true });
const temporary = await mkdtemp(new URL("download-", runtime));
try {
  const zip = `${temporary}/browser.zip`;
  const response = await fetch(pin.url, {
    signal: AbortSignal.timeout(120_000),
  });
  assert.equal(response.ok, true, `Browser download: HTTP ${response.status}`);
  await pipeline(response.body, createWriteStream(zip));
  assert.equal(
    createHash("sha256")
      .update(await readFile(zip))
      .digest("hex"),
    pin.sha256,
    "Browser archive checksum differs; refusing extraction.",
  );
  execFileSync("python3", ["-m", "zipfile", "-e", zip, runtime.pathname], {
    stdio: "inherit",
  });
  await chmod(
    new URL("chrome-headless-shell-linux64/chrome-headless-shell", runtime),
    0o755,
  );
  console.log(`Verified Chromium ${pin.version} in ${runtime.pathname}`);
} finally {
  await rm(temporary, { recursive: true, force: true });
}
