// the page's rule for views that reach it out of order, run with `mise run test:app-frontend`.

import assert from "node:assert/strict";
import test from "node:test";

import { newestFirst } from "../../frontend/revision.js";

test("a view is drawn when it is newer than the last", () => {
  const draws = newestFirst();
  assert.equal(draws(1), true);
  assert.equal(draws(2), true);
  assert.equal(draws(5), true);
});

test("a view older than the one drawn is dropped, and does not lower the newest", () => {
  const draws = newestFirst();
  assert.equal(draws(4), true);
  assert.equal(draws(3), false);
  assert.equal(draws(3), false);
  assert.equal(draws(4), true);
  assert.equal(draws(5), true);
});

test("a view of the revision already drawn is drawn again, as an event and a reply can carry one view", () => {
  const draws = newestFirst();
  assert.equal(draws(7), true);
  assert.equal(draws(7), true);
});
