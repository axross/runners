// the bundled page, real CSS and Chromium accessibility tree over Rust-generated
// views and shaped command fixtures. Native IPC and assistive technology stay separate.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { chromium } from "playwright-core";
import { bridge } from "./bridge.mjs";
import { accessibility, geometryAndContrast } from "./checks.mjs";

const root = fileURLToPath(new URL("../../../../../", import.meta.url));
const frontend = path.join(root, "app/crates/app/frontend");
const output = path.join(root, "app/target/render/output");
const pin = JSON.parse(
  await readFile(new URL("browser.json", import.meta.url)),
);
const policy = JSON.parse(
  await readFile(path.join(root, "app/crates/app/tauri.conf.json")),
).app.security.csp;
const key = (page, name) => page.locator(`[data-key=${JSON.stringify(name)}]`);
const active = (page) =>
  page.evaluate(() => document.activeElement?.dataset.key);
const scenarios = [
  "default",
  "edit",
  "removing",
  "host",
  "checking",
  "review",
  "review-tokens",
  "applied",
  "refused",
  "changed",
  "unwritable",
  "lost",
  "invalid",
  "omitted",
  "unusable",
  "missing",
  "too-large",
  "busy",
  "stopped",
];
const results = [];
await mkdir(output, { recursive: true });

/** builds both profiles' editor metadata from the real Rust view and recorded snapshots. */
function recordedViews(profile) {
  return Object.fromEntries(
    ["running", "stopped", "busy", "lost"].map((state) => {
      const fixture =
        { stopped: "stopped_by_you", busy: "resuming" }[state] ?? "running";
      const options =
        state === "lost" ? ["--connection", "dropped", "--keep-snapshot"] : [];
      return [
        state,
        JSON.parse(
          execFileSync(
            "cargo",
            [
              "run",
              "--locked",
              "--quiet",
              "-p",
              "marshal-view",
              "--example",
              "dump_view",
              "--",
              `crates/view/tests/fixtures/${fixture}.json`,
              "--editor-profile",
              profile,
              ...options,
            ],
            { cwd: path.join(root, "app"), encoding: "utf8" },
          ),
        ),
      ];
    }),
  );
}

const views = { axpc: recordedViews("axpc"), axmc: recordedViews("axmc") };
const browser = await chromium.launch({
  headless: true,
  executablePath: path.join(
    root,
    `app/target/render/browser-${pin.version}/chrome-headless-shell-linux64/chrome-headless-shell`,
  ),
});
assert.equal(browser.version(), pin.version);

/** routes only bundled files on a reserved origin; no server or external request is needed. */
async function open({
  profile,
  width,
  theme,
  forcedColors,
  scenario,
  mutation = null,
}) {
  const context = await browser.newContext({
    viewport: { width, height: 680 },
    colorScheme: theme,
    forcedColors,
  });
  const page = await context.newPage();
  const failures = [];
  page.on("pageerror", (error) => failures.push(String(error)));
  page.on("console", (message) => {
    if (message.type() === "error") failures.push(message.text());
  });
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    const file = path.resolve(
      frontend,
      `.${url.pathname === "/" ? "/index.html" : url.pathname}`,
    );
    const type = {
      ".html": "text/html",
      ".js": "text/javascript",
      ".css": "text/css",
      ".woff2": "font/woff2",
    }[path.extname(file)];
    if (
      url.origin !== "http://marshal-render.invalid" ||
      !file.startsWith(`${frontend}/`) ||
      !type
    ) {
      failures.push(`Unexpected resource: ${url}`);
      return route.abort();
    }
    let body = await readFile(file);
    if (file.endsWith("main.css") && mutation)
      body = Buffer.concat([body, Buffer.from(mutation)]);
    await route.fulfill({
      body,
      contentType: type,
      headers: file.endsWith("index.html")
        ? { "Content-Security-Policy": policy }
        : {},
    });
  });
  await page.addInitScript(bridge, { views: views[profile], scenario });
  await page.goto("http://marshal-render.invalid/");
  await key(
    page,
    ["unusable", "missing", "too-large"].includes(scenario)
      ? "read-config"
      : "edit-pool:0",
  ).waitFor();
  await page.evaluate(() => document.fonts.ready);
  return { page, context, failures };
}

/** reaches the named state by the page's own controls, not by replacing its DOM. */
async function enter(page, scenario) {
  if (["default", "unusable", "missing", "too-large"].includes(scenario))
    return;
  if (scenario === "host") {
    await key(page, "host-settings").click();
    const budget = key(page, "config-field:host:budget.disk_gb");
    await (
      (await budget.count())
        ? budget
        : key(page, "config-field:host:image_name")
    ).focus();
    return;
  }
  await key(page, "edit-pool:0").click();
  if (["invalid", "omitted"].includes(scenario)) {
    await key(
      page,
      `config-field:0:${scenario === "invalid" ? "runners" : "labels"}`,
    ).focus();
    return;
  }
  await key(page, "config-field:0:labels").fill("linux\nx64\nchanged");
  if (scenario === "refused")
    await key(page, "config-field:0:runners").fill("80");
  if (["lost", "busy"].includes(scenario)) {
    await page.evaluate((state) => window.__fixture.emit(state), scenario);
    return;
  }
  if (scenario === "edit") return;
  if (["review-tokens", "removing"].includes(scenario)) {
    await key(page, "edit-pool:2").click();
    await key(page, "remove-pool:2").click();
  }
  if (scenario === "removing") return;
  await key(page, "check-config").click();
  if (scenario === "checking") {
    assert.equal(
      await key(page, "check-config").getAttribute("aria-disabled"),
      "true",
    );
    return;
  }
  if (["refused", "changed"].includes(scenario)) {
    await page.locator(".config-notice").scrollIntoViewIfNeeded();
    return;
  }
  await key(page, "cancel-config").waitFor({ state: "visible" });
  assert.equal(await active(page), "cancel-config");
  if (["applied", "unwritable"].includes(scenario)) {
    await page.evaluate(() => {
      window.__fixture.holdWrite = true;
    });
    await page.mouse.move(0, 0);
    await key(page, "confirm-config").focus();
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => window.__fixture.release);
    assert.equal(await active(page), "confirm-config");
    await page.evaluate(() => window.__fixture.release());
    await key(page, "cancel-config").waitFor({ state: "detached" });
    await page.waitForFunction(() =>
      /Saved|unapplied changes are kept/.test(
        document.querySelector(".config-notice")?.textContent,
      ),
    );
    assert.equal(await active(page), "confirm-config");
    assert.equal(await key(page, "confirm-config").innerText(), "Done");
    assert.equal(await key(page, "confirm-config").count(), 1);
    if (scenario === "unwritable") {
      assert.match(
        await page.locator(".config-notice").innerText(),
        /access_refused/,
      );
      assert.equal(
        await key(page, "config-field:0:labels").inputValue(),
        "linux\nx64\nchanged",
      );
      assert.equal(
        await page.evaluate(
          () =>
            window.__fixture.calls.filter(
              (call) => call.request?.check === false,
            ).length,
        ),
        1,
      );
    }
  }
}

/** checks keyboard tooltips on each field kind without replacing its problem description. */
async function lockedTooltips(page, label) {
  for (const name of ["name", "labels", "diagnostics"]) {
    const field = key(page, `config-field:0:${name}`);
    await field.scrollIntoViewIfNeeded();
    const before = await field.getAttribute("aria-describedby");
    await page.mouse.move(0, 0);
    await page.keyboard.press("Tab");
    await field.focus();
    assert.equal(
      await field.evaluate((element) => element.matches(":focus-visible")),
      true,
    );
    assert.match(
      await page.getByRole("tooltip").innerText(),
      /Finish or cancel/,
    );
    await geometryAndContrast(page);
    await page.screenshot({
      path: path.join(output, `${label}-${name}-tooltip.png`),
    });
    await page.keyboard.press("Escape");
    assert.equal(await page.getByRole("tooltip").count(), 0);
    assert.equal(await field.getAttribute("aria-describedby"), before);
    await field.blur();
    await field.hover();
    assert.match(
      await page.getByRole("tooltip").innerText(),
      /Finish or cancel/,
    );
    await page.keyboard.press("Escape");
    await page.mouse.move(0, 0);
  }
  await key(page, "cancel-config").focus();
}

/** the opt-in choice cannot change while confirmation waits for the agent. */
async function waitingTokenChoice(page, label) {
  await page.evaluate(() => {
    window.__fixture.hold = true;
  });
  await key(page, "confirm-config").click();
  const choice = key(page, "release-token:owner/site");
  assert.equal(await choice.getAttribute("aria-disabled"), "true");
  await page.mouse.move(0, 0);
  await page.keyboard.press("Tab");
  await choice.focus();
  await page.keyboard.press("Space");
  assert.equal(await choice.isChecked(), false);
  const client = await page.context().newCDPSession(page);
  const { nodes } = await client.send("Accessibility.getFullAXTree");
  const checkbox = nodes.find((node) => node.role?.value === "checkbox");
  assert.match(
    checkbox.name.value,
    /^Also remove the stored token for owner\/site\./,
  );
  assert.equal(checkbox.description.value, "Waiting for the agent's answer.");
  await client.detach();
  assert.equal(
    await page.getByRole("tooltip").innerText(),
    checkbox.description.value,
  );
  const geometry = await geometryAndContrast(page);
  await page.screenshot({ path: path.join(output, `${label}-waiting.png`) });
  await page.keyboard.press("Escape");
  assert.equal(await page.getByRole("tooltip").count(), 0);
  await choice.blur();
  await choice.hover();
  assert.equal(
    await page.getByRole("tooltip").innerText(),
    checkbox.description.value,
  );
  await page.keyboard.press("Escape");
  return { label: `${label}-waiting`, ...geometry };
}

try {
  for (const width of [880, 480]) {
    for (const action of ["check-config", "reload-config"])
      for (const refusal of ["refuseCheck", "refuseWrite"]) {
        const label = `axpc-${action}-${refusal}-${width}-light-none`;
        const { page, context, failures } = await open({
          profile: "axpc",
          width,
          theme: "light",
          forcedColors: "none",
          scenario: "edit",
        });
        try {
          if (action === "check-config") await enter(page, "edit");
          await key(page, action).click();
          await key(page, "cancel-config").waitFor({ state: "visible" });
          await page.evaluate((name) => {
            window.__fixture[name] = true;
          }, refusal);
          await page.mouse.move(0, 0);
          await key(page, "confirm-config").focus();
          await page.keyboard.press("Enter");
          await page.waitForFunction(() =>
            document
              .querySelector(".config-notice")
              ?.textContent.includes("unapplied changes are kept"),
          );
          assert.equal(await active(page), "confirm-config");
          assert.equal(await key(page, "confirm-config").innerText(), "Done");
          assert.match(
            await page.locator(".config-review").innerText(),
            /Configuration not applied/,
          );
          assert.equal(
            await page.evaluate(
              () =>
                window.__fixture.calls.filter(
                  (call) => call.request?.check === false,
                ).length,
            ),
            refusal === "refuseWrite" ? 1 : 0,
          );
          if (action === "check-config") {
            assert.equal(
              await key(page, "config-field:0:labels").inputValue(),
              "linux\nx64\nchanged",
            );
            if (refusal === "refuseCheck") {
              await key(page, "read-again-config").click();
              await key(page, "keep-editing").waitFor({ state: "visible" });
              await page.mouse.move(0, 0);
              await page.keyboard.press("Escape");
              assert.equal(await active(page), "read-again-config");
              assert.equal(await key(page, "confirm-config").count(), 1);
              await key(page, "confirm-config").focus();
            }
          }
          const geometry = await geometryAndContrast(page);
          const tree = await accessibility(page);
          await page.screenshot({ path: path.join(output, `${label}.png`) });
          results.push({ label, ...geometry, ...tree });
          await page.keyboard.press("Enter");
          assert.equal(await active(page), "heading");
          assert.equal(await key(page, "confirm-config").count(), 0);
          assert.deepEqual(failures, []);
        } finally {
          await context.close();
        }
      }

    const label = `axpc-token-retry-${width}-dark-none`;
    const { page, context, failures } = await open({
      profile: "axpc",
      width,
      theme: "dark",
      forcedColors: "none",
      scenario: "review-tokens",
    });
    try {
      await enter(page, "review-tokens");
      await key(page, "release-token:owner/site").check();
      await page.evaluate(() => {
        window.__fixture.refuseRemoval = true;
      });
      await key(page, "confirm-config").click();
      await key(page, "retry-token:owner/site").waitFor({ state: "visible" });
      await page.waitForFunction(() =>
        document.querySelector(".config-notice")?.textContent.includes("Saved"),
      );
      assert.equal(
        await page.locator(".config-token-failure p").innerText(),
        "The token for owner/site was not removed. Access refused. The configuration stays saved.",
      );
      await page.evaluate(() => {
        window.__fixture.refuseRemoval = false;
        window.__fixture.holdRemoval = true;
      });
      await page.mouse.move(0, 0);
      await key(page, "retry-token:owner/site").focus();
      await page.keyboard.press("Enter");
      await page.waitForFunction(() => window.__fixture.release);
      assert.equal(await active(page), "retry-token:owner/site");
      assert.equal(
        await key(page, "retry-token:owner/site").getAttribute("aria-disabled"),
        "true",
      );
      assert.equal(
        await key(page, "add-pool").getAttribute("aria-disabled"),
        "true",
      );
      await page.keyboard.press("Enter");
      await page.keyboard.press("Space");
      await page.evaluate(() => window.__fixture.release());
      await page.waitForFunction(() =>
        document
          .querySelector(".config-token-failure")
          ?.textContent.includes("was removed"),
      );
      assert.equal(await active(page), "retry-token:owner/site");
      assert.equal(
        await key(page, "retry-token:owner/site").innerText(),
        "Removed",
      );
      await page.keyboard.press("Enter");
      await page.keyboard.press("Space");
      assert.deepEqual(
        await page.evaluate(() => ({
          writes: window.__fixture.calls.filter(
            (call) => call.request?.check === false,
          ).length,
          removals: window.__fixture.calls.filter(
            (call) => call.request?.kind === "remove_released_token",
          ).length,
        })),
        { writes: 1, removals: 2 },
      );
      const geometry = await geometryAndContrast(page);
      await page.screenshot({ path: path.join(output, `${label}.png`) });
      results.push({ label, ...geometry });
      assert.deepEqual(failures, []);
    } finally {
      await context.close();
    }

    const reusedLabel = `axpc-token-reused-${width}-light-none`;
    const reused = await open({
      profile: "axpc",
      width,
      theme: "light",
      forcedColors: "none",
      scenario: "review-tokens",
    });
    try {
      const page = reused.page;
      const retry = key(page, "retry-token:owner/site");
      const removals = () =>
        page.evaluate(
          () =>
            window.__fixture.calls.filter(
              (call) => call.request?.kind === "remove_released_token",
            ).length,
        );
      await enter(page, "review-tokens");
      await key(page, "release-token:owner/site").check();
      await page.evaluate(() => {
        window.__fixture.refuseRemoval = true;
      });
      await key(page, "confirm-config").click();
      await page.waitForFunction(() =>
        document.querySelector(".config-notice")?.textContent.includes("Saved"),
      );
      assert.equal(await retry.innerText(), "Try Again");
      assert.equal(await removals(), 1);
      await key(page, "add-pool").click();
      await page
        .locator('[data-key^="config-field:"][data-key$=":name"]')
        .fill("returned");
      await page
        .locator('[data-key^="config-field:"][data-key$=":repository"]')
        .fill("Owner/Site");
      await key(page, "check-config").click();
      await key(page, "cancel-config").waitFor({ state: "visible" });
      assert.equal(await retry.getAttribute("aria-disabled"), "true");
      await page.evaluate(() => {
        window.__fixture.holdWrite = true;
      });
      await key(page, "confirm-config").click();
      await page.waitForFunction(() => window.__fixture.release);
      await retry.focus();
      assert.equal(await retry.getAttribute("aria-disabled"), "true");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Space");
      await retry.dispatchEvent("click");
      assert.equal(await removals(), 1);
      await geometryAndContrast(page);
      await page.screenshot({
        path: path.join(output, `${reusedLabel}-waiting.png`),
      });
      await page.evaluate(() => window.__fixture.release());
      await page.waitForFunction(
        () =>
          document.querySelector('[data-key="retry-token:owner/site"]')
            ?.textContent === "Cancelled",
      );
      assert.equal(await active(page), "retry-token:owner/site");
      assert.equal(await retry.getAttribute("aria-disabled"), "true");
      assert.ok(await retry.getAttribute("aria-describedby"));
      await page.keyboard.press("Enter");
      await page.keyboard.press("Space");
      await retry.dispatchEvent("click");
      assert.equal(await removals(), 1);
      await page.keyboard.press("Escape");
      await page.mouse.move(0, 0);
      const geometry = await geometryAndContrast(page);
      const tree = await accessibility(page);
      await page.screenshot({ path: path.join(output, `${reusedLabel}.png`) });
      results.push({ label: reusedLabel, ...geometry, ...tree });
      assert.deepEqual(reused.failures, []);
    } finally {
      await reused.context.close();
    }
  }

  for (const profile of ["axpc", "axmc"])
    for (const width of [880, 480])
      for (const theme of ["light", "dark"])
        for (const forcedColors of ["none", "active"])
          for (const scenario of scenarios) {
            const label = `${profile}-${scenario}-${width}-${theme}-${forcedColors}`;
            const { page, context, failures } = await open({
              profile,
              width,
              theme,
              forcedColors,
              scenario,
            });
            try {
              await enter(page, scenario);
              await page.mouse.move(0, 0);
              if (["missing", "unusable", "too-large"].includes(scenario)) {
                const explanation = await page
                  .locator(".config-notice p.config-error")
                  .first()
                  .innerText();
                assert.equal(
                  await page
                    .locator(".scroller p")
                    .filter({ hasText: explanation })
                    .count(),
                  1,
                );
                const host = key(page, "host-settings");
                assert.equal(await host.getAttribute("aria-disabled"), "true");
                await page.keyboard.press("Tab");
                await host.focus();
                const reason = await page.getByRole("tooltip").innerText();
                assert.match(reason, /text editor/);
                const description = await host.getAttribute("aria-describedby");
                assert.equal(
                  await page
                    .locator(`[id=${JSON.stringify(description)}]`)
                    .innerText(),
                  reason,
                );
                await page.keyboard.press("Enter");
                await page.keyboard.press("Space");
                assert.equal(await host.innerText(), "Edit Host Settings");
                assert.equal(
                  await page
                    .getByRole("region", { name: "Host settings" })
                    .count(),
                  0,
                );
                await page.keyboard.press("Escape");
                assert.equal(await page.getByRole("tooltip").count(), 0);
              }
              if (scenario === "review") await lockedTooltips(page, label);
              const geometry = await geometryAndContrast(page);
              const tree = await accessibility(page);
              assert.deepEqual(
                failures,
                [],
                `${label}: page or policy failure`,
              );
              await page.screenshot({
                path: path.join(output, `${label}.png`),
              });
              results.push({ label, ...geometry, ...tree });
              if (
                scenario === "removing" &&
                profile === "axpc" &&
                width === 480 &&
                theme === "light" &&
                forcedColors === "none"
              ) {
                await page
                  .locator(".config-removed h2")
                  .evaluate((element) =>
                    element.setAttribute("role", "presentation"),
                  );
                await assert.rejects(
                  accessibility(page),
                  /pool missing its heading role: site/,
                );
              }
              if (scenario === "applied") {
                await page.keyboard.press("Enter");
                assert.equal(await active(page), "heading");
                assert.equal(await key(page, "confirm-config").count(), 0);
              }
              if (scenario === "review-tokens") {
                results.push(await waitingTokenChoice(page, label));
                assert.deepEqual(
                  failures,
                  [],
                  `${label}: pending review failure`,
                );
              }
            } catch (error) {
              await page.screenshot({
                path: path.join(output, `${label}-failure.png`),
              });
              throw new Error(label, { cause: error });
            } finally {
              await context.close();
            }
          }

  for (const width of [880, 480])
    for (const scenario of ["saved-unreadable", "saved-token-wait"]) {
      const label = `axpc-${scenario}-${width}-dark-none`;
      const { page, context, failures } = await open({
        profile: "axpc",
        width,
        theme: "dark",
        forcedColors: "none",
        scenario: scenario === "saved-unreadable" ? "applied" : "review-tokens",
      });
      try {
        if (scenario === "saved-unreadable") {
          await page.evaluate(() => {
            window.__fixture.failReadAfterWrite = true;
          });
          await enter(page, "applied");
          assert.match(
            await page.locator(".config-notice").innerText(),
            /Saved, but config\.toml could not be read again\. Lost contact\. Read/,
          );
        } else {
          await enter(page, "review-tokens");
          await key(page, "release-token:owner/site").check();
          await page.evaluate(() => {
            window.__fixture.holdRemoval = true;
          });
          await page.mouse.move(0, 0);
          await key(page, "confirm-config").focus();
          await page.keyboard.press("Enter");
          await page.waitForFunction(() => window.__fixture.release);
          assert.equal(await active(page), "confirm-config");
          assert.equal(await key(page, "confirm-config").innerText(), "Done");
          assert.equal(
            await key(page, "confirm-config").getAttribute("aria-disabled"),
            "true",
          );
          await page.keyboard.press("Enter");
          assert.equal(await active(page), "confirm-config");
          const pending = await geometryAndContrast(page);
          await page.screenshot({
            path: path.join(output, `${label}-pending.png`),
          });
          results.push({ label: `${label}-pending`, ...pending });
          await page.evaluate(() => window.__fixture.release());
          await page.waitForFunction(() =>
            document
              .querySelector(".config-notice")
              ?.textContent.includes("Saved"),
          );
        }
        assert.equal(await active(page), "confirm-config");
        assert.equal(await key(page, "confirm-config").count(), 1);
        const geometry = await geometryAndContrast(page);
        await page.screenshot({ path: path.join(output, `${label}.png`) });
        results.push({ label, ...geometry });
        await page.keyboard.press("Enter");
        assert.equal(await active(page), "heading");
        assert.equal(await key(page, "confirm-config").count(), 0);
        if (scenario === "saved-token-wait") {
          await key(page, "reload-config").click();
          await key(page, "cancel-config").waitFor({ state: "visible" });
          assert.equal(await key(page, "confirm-config").count(), 1);
          assert.notEqual(
            await key(page, "confirm-config").innerText(),
            "Done",
          );
          await page.mouse.move(0, 0);
          await page.keyboard.press("Escape");
          await page.locator('[data-key^="edit-pool:"]').first().click();
          assert.equal(await key(page, "confirm-config").count(), 0);
        }
        assert.deepEqual(failures, []);
      } finally {
        await context.close();
      }
    }

  for (const width of [880, 480]) {
    for (const action of ["reload-config", "check-config"]) {
      const { page, context, failures } = await open({
        profile: "axpc",
        width,
        theme: "light",
        forcedColors: "none",
        scenario: "edit",
      });
      if (action === "check-config") await enter(page, "edit");
      await key(page, "tab:tokens").click();
      await key(page, action).click();
      await key(page, "cancel-config").waitFor({ state: "visible" });
      assert.equal(await active(page), "cancel-config");
      assert.equal(
        await key(page, "tab:pools").getAttribute("aria-selected"),
        "true",
      );
      await geometryAndContrast(page);
      await page.mouse.move(0, 0);
      await page.keyboard.press("Escape");
      assert.equal(await active(page), action);
      assert.equal(await key(page, "confirm-config").count(), 0);
      assert.equal(
        await page.evaluate(() =>
          window.__fixture.calls.some((call) => call.request?.check === false),
        ),
        false,
      );
      assert.deepEqual(failures, []);
      await context.close();
    }

    const { page, context } = await open({
      profile: "axpc",
      width,
      theme: "light",
      forcedColors: "none",
      scenario: "stopped",
    });
    await enter(page, "stopped");
    assert.match(
      await page.locator(".config-review").innerText(),
      /next Start All/,
    );
    assert.doesNotMatch(
      await page.locator(".config-review").innerText(),
      /jobs? .*fails/,
    );
    await key(page, "start_all").click();
    assert.equal(await key(page, "confirm-config").count(), 0);
    assert.equal(
      await page.evaluate(() =>
        window.__fixture.calls.some((call) => call.request?.check === false),
      ),
      false,
    );
    await key(page, "check-config").click();
    assert.match(
      await page.locator(".config-review").innerText(),
      /job running on them fails/,
    );
    await page.mouse.move(0, 0);
    await page.keyboard.press("Escape");
    assert.equal(await active(page), "check-config");
    await context.close();
  }

  for (const [mutation, expected] of [
    [".config-input { width: 1500px !important; }", /overflow/],
    [
      ".config-input { color: #eee !important; background: #eee !important; }",
      /text contrast/,
    ],
    [null, /label|accessible name/],
  ]) {
    const { page, context } = await open({
      profile: "axpc",
      width: 480,
      theme: "light",
      forcedColors: "none",
      scenario: "edit",
      mutation,
    });
    await enter(page, "edit");
    if (mutation) await assert.rejects(geometryAndContrast(page), expected);
    else {
      await key(page, "config-field:0:name").evaluate((element) =>
        element.setAttribute("aria-label", "broken name"),
      );
      await assert.rejects(accessibility(page), expected);
    }
    await context.close();
  }
  console.log(
    `Chromium ${browser.version()}: ${results.length} rendered states passed; overflow, contrast, label and removed-pool heading mutation controls failed as required.`,
  );
} finally {
  await writeFile(
    path.join(output, "results.json"),
    `${JSON.stringify({ browser: browser.version(), results }, null, 2)}\n`,
  );
  await browser.close();
}
