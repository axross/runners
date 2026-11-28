import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const source = new URL("../", import.meta.url);

function setup(t, checkoutName, { rustStatus = 0, copyFails = false } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "claude-start-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const checkout = join(dir, checkoutName);
  const claude = join(checkout, ".claude");
  const bin = join(dir, "bin");
  const home = join(dir, "home");
  const agents = join(checkout, ".agents");
  mkdirSync(claude, { recursive: true });
  for (const path of [agents, bin, home]) mkdirSync(path);
  for (const name of [
    "hooks",
    "settings.json",
    "settings.local-example.json",
  ]) {
    cpSync(new URL(name, source), join(claude, name), { recursive: true });
  }

  symlinkSync("/usr/bin/grep", join(bin, "grep"));
  if (copyFails) {
    writeFileSync(
      join(bin, "cp"),
      "#!/bin/sh\necho 'copy failed' >&2\nexit 42\n",
      {
        mode: 0o755,
      },
    );
  } else {
    symlinkSync("/bin/cp", join(bin, "cp"));
  }
  const trace = join(dir, "provisioning.log");
  writeFileSync(join(bin, "mise"), '#!/bin/sh\necho "mise $*" >> "$TRACE"\n', {
    mode: 0o755,
  });
  writeFileSync(
    join(agents, "install-rust"),
    '#!/bin/sh\necho rust >> "$TRACE"\nexit "$RUST_STATUS"\n',
    { mode: 0o755 },
  );

  const settings = JSON.parse(
    readFileSync(join(claude, "settings.json"), "utf8"),
  );
  const hooks = settings.hooks.SessionStart.filter(({ matcher }) =>
    new RegExp(matcher || "").test("startup"),
  ).flatMap((group) => group.hooks);
  assert.equal(hooks.length, 2);
  const commandFor = (file) => {
    const hook = hooks.find(({ command }) => command?.includes(file));
    assert.equal(hook?.type, "command", file);
    return hook.command;
  };

  const envFile = join(dir, "session.env");
  function run(command, remote) {
    const result = spawnSync("/bin/sh", ["-c", command], {
      cwd: dir,
      encoding: "utf8",
      env: {
        HOME: home,
        PATH: bin,
        CLAUDE_ENV_FILE: envFile,
        CLAUDE_PROJECT_DIR: checkout,
        ...(remote && { CLAUDE_CODE_REMOTE: "true" }),
        TRACE: trace,
        RUST_STATUS: String(rustStatus),
      },
    });
    assert.ifError(result.error);
    return result;
  }

  return {
    reminder: (remote) => run(commandFor("session-guidance.sh"), remote),
    bootstrap: (remote) => run(commandFor("session-start.sh"), remote),
    trace,
    envFile,
    settings: join(claude, "settings.local.json"),
    example: join(claude, "settings.local-example.json"),
  };
}

function checkReminder(result) {
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
  const lines = result.stdout.trimEnd().split("\n");
  assert.equal(lines.length, 1);
  for (const instruction of [
    "AGENTS.md",
    "Response Approach",
    "Before planning, delegating or editing",
    "Loop Engineering",
  ]) {
    assert.ok(lines[0].includes(instruction), result.stdout);
  }
}

function checkProvisioning(fixture) {
  assert.deepEqual(readFileSync(fixture.trace, "utf8").trimEnd().split("\n"), [
    "mise activate bash",
    "mise trust",
    "mise install",
    "rust",
  ]);
  const env = readFileSync(fixture.envFile, "utf8");
  assert.ok(env.includes('eval "$(mise activate bash)"'));
  assert.ok(env.includes('export PATH="$HOME/.cargo/bin:$PATH"'));
}

for (const rustStatus of [0, 1]) {
  test(
    rustStatus
      ? "remote startup with a Rust installer warning"
      : "normal remote startup",
    (t) => {
      const fixture = setup(t, "runners checkout", { rustStatus });
      checkReminder(fixture.reminder(true));
      assert.ok(!existsSync(fixture.trace));
      assert.ok(!existsSync(fixture.envFile));
      assert.ok(!existsSync(fixture.settings));

      const result = fixture.bootstrap(true);
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout, "");
      if (rustStatus) {
        assert.match(
          result.stderr,
          /^warning: \.agents\/install-rust failed[^\n]*\n$/,
        );
      } else {
        assert.equal(result.stderr, "");
      }
      checkProvisioning(fixture);
      assert.deepEqual(
        readFileSync(fixture.settings),
        readFileSync(fixture.example),
      );
    },
  );
}

for (const name of ["runners", "runners checkout"]) {
  test(`failed settings copy in "${name}" preserves status and independent reminder`, (t) => {
    const fixture = setup(t, name, { copyFails: true });
    const result = fixture.bootstrap(true);
    assert.equal(result.status, 42, result.stderr);
    assert.equal(result.stdout, "");
    assert.equal(result.stderr, "copy failed\n");
    checkProvisioning(fixture);
    assert.ok(!existsSync(fixture.settings));
    checkReminder(fixture.reminder(true));
  });
}

test("local startup without opt-in is silent and skips provisioning", (t) => {
  const fixture = setup(t, "runners checkout");
  for (const result of [fixture.reminder(false), fixture.bootstrap(false)]) {
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, "");
    assert.equal(result.stderr, "");
  }
  assert.ok(!existsSync(fixture.trace));
  assert.ok(!existsSync(fixture.envFile));
  assert.ok(!existsSync(fixture.settings));
});
