// checks the browser's layout, computed colours and accessibility tree, rather
// than inferring any of them from the page's document-building stand-in.

import assert from "node:assert/strict";

/** checks all laid-out text, input boundaries and the current keyboard focus. */
export async function geometryAndContrast(page) {
  const result = await page.evaluate(() => {
    const rgb = (text) => {
      const parts = text.match(/[\d.]+/g).map(Number);
      return [...parts.slice(0, 3), parts[3] ?? 1];
    };
    const over = (a, b) =>
      a.slice(0, 3).map((channel, i) => channel * a[3] + b[i] * (1 - a[3]));
    const background = (element) => {
      const parents = [];
      for (let node = element; node; node = node.parentElement)
        parents.unshift(node);
      return parents.reduce(
        (colour, node) =>
          over(rgb(getComputedStyle(node).backgroundColor), colour),
        [255, 255, 255],
      );
    };
    const luminance = (colour) =>
      colour
        .map((channel) => {
          const value = channel / 255;
          return value <= 0.04045
            ? value / 12.92
            : ((value + 0.055) / 1.055) ** 2.4;
        })
        .reduce(
          (sum, value, i) => sum + value * [0.2126, 0.7152, 0.0722][i],
          0,
        );
    const contrast = (a, b) => {
      const values = [luminance(a), luminance(b)].sort((x, y) => x - y);
      return (values[1] + 0.05) / (values[0] + 0.05);
    };
    const issues = [];
    const width = document.documentElement.clientWidth;
    let lowestText = Infinity;
    let lowestBorder = Infinity;
    for (const element of document.querySelectorAll("#app *")) {
      if (element.closest(".vh") || !element.getClientRects().length) continue;
      const rect = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      const name = element.dataset.key ?? element.className;
      if (
        rect.left < -1 ||
        rect.right > width + 1 ||
        (element.scrollWidth > element.clientWidth + 2 &&
          style.overflowX === "hidden")
      )
        issues.push(`overflow: ${name}`);
      const hasText = [...element.childNodes].some(
        (node) => node.nodeType === Node.TEXT_NODE && node.textContent.trim(),
      );
      if (
        hasText ||
        element.matches("input:not([type=checkbox]),textarea,select")
      ) {
        const ratio = contrast(
          over(rgb(style.color), background(element)),
          background(element),
        );
        lowestText = Math.min(lowestText, ratio);
        if (ratio < 4.5)
          issues.push(`text contrast ${ratio.toFixed(2)}: ${name}`);
      }
      if (element.matches(".config-input")) {
        const border = rgb(style.borderTopColor);
        const ratio = Math.min(
          contrast(over(border, background(element)), background(element)),
          contrast(
            over(border, background(element.parentElement)),
            background(element.parentElement),
          ),
        );
        lowestBorder = Math.min(lowestBorder, ratio);
        if (parseFloat(style.borderTopWidth) === 0 || ratio < 3)
          issues.push(`border contrast ${ratio.toFixed(2)}: ${name}`);
      }
    }
    const focus = document.activeElement;
    let focusContrast = null;
    if (focus?.matches(":focus-visible") && focus.closest("#app")) {
      const style = getComputedStyle(focus);
      const behind = background(focus.parentElement);
      focusContrast = contrast(over(rgb(style.outlineColor), behind), behind);
      if (
        style.outlineStyle === "none" ||
        parseFloat(style.outlineWidth) < 3 ||
        focusContrast < 3
      )
        issues.push(`focus contrast ${focusContrast}: ${focus.dataset.key}`);
    }
    const tooltip = document.querySelector('[role="tooltip"]');
    if (tooltip) {
      const box = tooltip.getBoundingClientRect();
      let top = 0;
      let bottom = innerHeight;
      for (let node = tooltip.parentElement; node; node = node.parentElement) {
        if (/auto|scroll|hidden/.test(getComputedStyle(node).overflowY)) {
          const clip = node.getBoundingClientRect();
          top = Math.max(top, clip.top);
          bottom = Math.min(bottom, clip.bottom);
        }
      }
      if (box.top < top - 1 || box.bottom > bottom + 1)
        issues.push(
          `tooltip clipped by its scroll region: ${box.top}-${box.bottom} in ${top}-${bottom}`,
        );
    }
    const footer = document.querySelector(".footer").getBoundingClientRect();
    const scroller = document
      .querySelector(".scroller")
      .getBoundingClientRect();
    const bottom =
      document.querySelector(".config-bar")?.getBoundingClientRect() ?? footer;
    if (document.documentElement.scrollWidth !== width)
      issues.push("overflow: document");
    if (
      footer.bottom > innerHeight + 1 ||
      scroller.height < 40 ||
      scroller.bottom > bottom.top + 1
    )
      issues.push(
        "vertical layout: unreachable footer or collapsed/overlaid pool area",
      );
    return {
      issues,
      lowestText,
      lowestBorder: Number.isFinite(lowestBorder) ? lowestBorder : null,
      focusContrast,
    };
  });
  assert.deepEqual(result.issues, [], result.issues.join("\n"));
  return result;
}

/** checks Chromium's names and descriptions against fields and the fixture's pool names. */
export async function accessibility(page, poolNames = ["app", "site", "docs"]) {
  const session = await page.context().newCDPSession(page);
  try {
    const { root } = await session.send("DOM.getDocument");
    const { nodeIds } = await session.send("DOM.querySelectorAll", {
      nodeId: root.nodeId,
      selector: ".config-input",
    });
    const { nodes } = await session.send("Accessibility.getFullAXTree");
    for (const nodeId of nodeIds) {
      const { node } = await session.send("DOM.describeNode", { nodeId });
      const attributes = Object.fromEntries(
        Array.from({ length: node.attributes.length / 2 }, (_, i) =>
          node.attributes.slice(i * 2, i * 2 + 2),
        ),
      );
      const expected = await page
        .locator(`[id=${JSON.stringify(attributes.id)}]`)
        .evaluate((element) => ({
          name: element.labels?.[0]?.textContent,
          description: document.getElementById(
            element.getAttribute("aria-describedby"),
          )?.textContent,
        }));
      assert.ok(expected.name, "field has no visible label");
      assert.ok(expected.description, "field has no description");
      const accessible = nodes.find(
        (entry) =>
          entry.backendDOMNodeId === node.backendNodeId && !entry.ignored,
      );
      assert.ok(
        accessible,
        `field missing from accessibility tree: ${expected.name}`,
      );
      assert.equal(
        accessible.name?.value,
        expected.name,
        "field accessible name differs from visible label",
      );
      assert.equal(
        accessible.description?.value,
        expected.description,
        "field accessible description lost its problem or reason",
      );
    }
    const headings = nodes
      .filter((node) => !node.ignored && node.role?.value === "heading")
      .map((node) => node.name?.value);
    for (const text of poolNames)
      assert.ok(
        headings.includes(text),
        `pool missing its heading role: ${text}`,
      );
    return { fields: nodeIds.length, headings: headings.length };
  } finally {
    await session.detach();
  }
}
