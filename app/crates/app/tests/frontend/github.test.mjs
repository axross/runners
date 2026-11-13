// the GitHub column of the main window's page, run with `mise run test:app-frontend`:
// each runner's GitHub state in words, Open run's role, Tab stop and accessible name,
// what Enter on it asks the app, where focus goes when its runner goes away, and the
// budget line. The page is loaded as it is, over a stand-in for the document and for
// the app's `invoke` and `listen`.

import assert from "node:assert/strict";
import test from "node:test";

import { Element, installDocument } from "./document.mjs";

const OPEN_NAME =
  "Open run of CI · test (ubuntu-latest), app, Runner 1, on GitHub";

function row(slot, container, github, job) {
  return {
    slot,
    name: `Runner ${slot}`,
    state: "Container running",
    mark: "check",
    container,
    since: "since 03:15",
    detail: null,
    label: `app, Runner ${slot}, container running, ${github}`,
    github,
    github_checked: null,
    job: job ?? null,
    open_run: job
      ? { label: "Open run", name: OPEN_NAME, runner: container }
      : null,
  };
}

let revision = 0;

/** a view of the app pool with the given runner rows. */
function shown(runners, { announce = [], notes = [] } = {}) {
  revision += 1;
  return {
    revision,
    announce,
    view: {
      window: {
        body: {
          kind: "list",
          header: {
            heading: "2 of 2 runners up",
            line: "Running since today 03:15 · 1 pool · 1 running job",
            controls: [],
            tokens_chip: null,
            opens_on: "pools",
          },
          banners: [],
          pools: [
            {
              name: "app",
              repository: "owner/app",
              state: "Running",
              mark: null,
              count: "2 of 2 up",
              tags: [],
              from: null,
              line: "Running · 2 of 2 up",
              note: "No token for owner/app.",
              pill: {
                action: { set_token: "owner/app" },
                label: "Set Token",
                available: true,
                look: "tonal",
                reason: null,
                confirmation: null,
              },
              github_notes: notes,
              runners,
              diagnostics: null,
            },
          ],
          pools_note: null,
          stale: false,
          tokens: { intro: "Tokens.", rows: [] },
        },
        footer: {
          github: [
            "GitHub requests: 4,210 of 5,000 left this hour · resets 04:05 · checked every 15 s",
          ],
          image_lines: ["Runner image: contract 3"],
          warnings: [],
          configuration: "Configuration: config.json",
          note: "A note.",
        },
      },
    },
  };
}

const BUSY = row(
  1,
  "app-1-20261104031522114",
  "GitHub: running a job",
  "CI · test (ubuntu-latest)",
);
const IDLE = row(2, "app-2-20261104031530871", "GitHub: idle");

const { app, live } = Object.fromEntries(
  [...installDocument(["app", "live", "layer"])].map(([id, element]) => [
    id,
    element,
  ]),
);

const calls = [];
const handlers = new Map();

globalThis.window = {
  __TAURI__: {
    core: {
      invoke: (command, args) => {
        calls.push({ command, args });
        return command === "read_view"
          ? Promise.resolve(shown([BUSY, IDLE]))
          : Promise.resolve(null);
      },
    },
    event: {
      listen: (name, handler) => {
        handlers.set(name, handler);
        return Promise.resolve(() => {});
      },
    },
  },
};

await import("../../frontend/main.js");

async function settle() {
  for (let turn = 0; turn < 4; turn += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

function elements() {
  return [...app.descendants()];
}

/** the text an element shows, its children's included. */
function text(element) {
  return [element.textContent]
    .concat(
      element.children.map((child) =>
        child instanceof Element ? text(child) : child.text,
      ),
    )
    .join("");
}

function links() {
  return elements().filter(
    (element) => element.getAttribute("role") === "link",
  );
}

/** the elements Tab stops on, in document order. */
function tabStops() {
  return elements().filter(
    (element) =>
      element.getAttribute("tabindex") === "0" ||
      (element.tag === "button" && element.getAttribute("tabindex") !== "-1"),
  );
}

await settle();

test("each runner's GitHub state reads as words in its row", () => {
  const rows = elements().filter((element) => element.tag === "li");
  assert.equal(rows.length, 2);
  assert.match(text(rows[0]), /GitHub: running a job/);
  assert.match(text(rows[0]), /CI · test \(ubuntu-latest\)/);
  assert.match(text(rows[1]), /GitHub: idle/);
  assert.doesNotMatch(text(rows[1]), /Open run/);
});

test("Open run is a link that takes Tab, after the card's pill, and names its job and runner", () => {
  const [link] = links();
  assert.equal(links().length, 1);
  assert.equal(link.getAttribute("tabindex"), "0");
  assert.equal(link.getAttribute("aria-label"), OPEN_NAME);
  assert.equal(link.textContent, "Open run");
  const stops = tabStops();
  const pill = stops.findIndex(
    (element) =>
      element.getAttribute("data-key") === "pill:set_token:owner/app",
  );
  assert.ok(pill >= 0, "the pill takes Tab");
  assert.ok(
    stops.indexOf(link) > pill,
    "the link comes after the pill, in list order",
  );
  const heading = elements().find(
    (element) => element.getAttribute("data-key") === "pool:app",
  );
  assert.equal(
    heading.getAttribute("tabindex"),
    "-1",
    "the heading never takes Tab",
  );
});

test("Enter on Open run asks the app to open that runner's job, naming no address", async () => {
  calls.length = 0;
  let prevented = false;
  links()[0].dispatchEvent({
    type: "keydown",
    key: "Enter",
    preventDefault: () => {
      prevented = true;
    },
  });
  await settle();
  const opened = calls.filter((call) => call.command === "open_job");
  assert.deepEqual(opened, [
    {
      command: "open_job",
      args: { pool: "app", runner: "app-1-20261104031522114" },
    },
  ]);
  assert.ok(prevented);
  assert.doesNotMatch(JSON.stringify(calls), /https?:/);
});

test("another key on Open run asks nothing", async () => {
  calls.length = 0;
  links()[0].dispatchEvent({ type: "keydown", key: " ", preventDefault() {} });
  await settle();
  assert.equal(calls.filter((call) => call.command === "open_job").length, 0);
});

test("an announcement is written to the live region without moving focus", () => {
  const link = links()[0];
  link.focus();
  handlers.get("marshal:view")({
    payload: shown([BUSY, IDLE], {
      announce: ["Pool app: That job has ended."],
      notes: [{ text: "That job has ended.", line: null }],
    }),
  });
  assert.equal(
    document.activeElement.getAttribute("data-key"),
    link.getAttribute("data-key"),
  );
  assert.match(text(live), /Pool app: That job has ended\./);
  assert.match(text(app), /That job has ended\./);
});

test("when a focused Open run goes away with its runner, focus moves to its pool's heading", () => {
  links()[0].focus();
  handlers.get("marshal:view")({ payload: shown([IDLE]) });
  assert.equal(links().length, 0);
  assert.equal(document.activeElement.getAttribute("data-key"), "pool:app");
});

test("the budget is the footer's first line", () => {
  const footer = elements().find((element) => element.tag === "footer");
  const first = footer.children.find((child) => child instanceof Element);
  assert.equal(
    first.textContent,
    "GitHub requests: 4,210 of 5,000 left this hour · resets 04:05 · checked every 15 s",
  );
});
