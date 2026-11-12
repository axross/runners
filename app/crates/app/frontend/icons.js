// the marks of the windows, drawn in Juicio's icon idiom: a 24 px grid, a 1.5 px
// round stroke in the current colour. Each icon is fixed data, so no markup is parsed.

const NAMESPACE = "http://www.w3.org/2000/svg";

const ICONS = {
  play: [["path", { d: "M7 4.75v14.5L19 12 7 4.75z" }]],
  square: [
    ["rect", { x: "5.5", y: "5.5", width: "13", height: "13", rx: "2" }],
  ],
  check: [["path", { d: "M20 6L9 17L4 12" }]],
  history: [
    ["path", { d: "M12 8V12L14 14" }],
    [
      "path",
      {
        d: "M3.05 11C3.27409 8.80006 4.30026 6.75961 5.93273 5.26797C7.5652 3.77632 9.68969 2.93788 11.9009 2.91263C14.1121 2.88737 16.2552 3.67706 17.9213 5.13103C19.5874 6.585 20.6599 8.60148 20.9342 10.7957C21.2085 12.99 20.6653 15.2084 19.4084 17.0278C18.1514 18.8471 16.2686 20.14 14.1193 20.6599C11.9699 21.1797 9.70441 20.89 7.755 19.8461C5.80558 18.8022 4.30872 17.0771 3.55 15M3.05 20V15H8.05",
      },
    ],
  ],
  alert: [
    ["path", { d: "M12 6.5V13" }],
    ["path", { d: "M12 17.25H12.01" }],
  ],
  warning: [
    [
      "path",
      {
        d: "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3",
      },
    ],
    ["path", { d: "M12 9v4" }],
    ["path", { d: "M12 17h.01" }],
  ],
  info: [
    ["circle", { cx: "12", cy: "12", r: "10" }],
    ["path", { d: "M12 16v-4" }],
    ["path", { d: "M12 8h.01" }],
  ],
  error: [
    ["circle", { cx: "12", cy: "12", r: "10" }],
    ["path", { d: "M12 8v4" }],
    ["path", { d: "M12 16h.01" }],
  ],
  slashed: [
    ["circle", { cx: "12", cy: "12", r: "10" }],
    ["path", { d: "M4.93 4.93l14.14 14.14" }],
  ],
  retry: [
    ["path", { d: "M3 12a9 9 0 1 0 2.64-6.36L3 8.25" }],
    ["path", { d: "M3 3.5v4.75h4.75" }],
  ],
  build: [
    ["path", { d: "M12 2.75l8.25 4.5v9.5L12 21.25l-8.25-4.5v-9.5z" }],
    ["path", { d: "M3.75 7.25L12 11.75l8.25-4.5" }],
    ["path", { d: "M12 11.75v9.5" }],
  ],
  dash: [["path", { d: "M8 12h8" }]],
  key: [
    [
      "path",
      { d: "m15.5 7.5 2.3 2.3a1 1 0 0 0 1.4 0l2.1-2.1a1 1 0 0 0 0-1.4L19 4" },
    ],
    ["path", { d: "m21 2-9.6 9.6" }],
    ["circle", { cx: "7.5", cy: "15.5", r: "5.5" }],
  ],
  trash: [
    ["path", { d: "M3 6h18" }],
    ["path", { d: "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" }],
    ["path", { d: "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" }],
  ],
};

/** an icon of the table as an SVG element that assistive technology skips. */
export function icon(name, size = 20, className = "") {
  const svg = document.createElementNS(NAMESPACE, "svg");
  const attributes = {
    "aria-hidden": "true",
    width: String(size),
    height: String(size),
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": "1.5",
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
  };
  for (const [key, value] of Object.entries(attributes)) {
    svg.setAttribute(key, value);
  }
  if (className) {
    svg.setAttribute("class", className);
  }
  for (const [tag, shape] of ICONS[name] ?? []) {
    const part = document.createElementNS(NAMESPACE, tag);
    for (const [key, value] of Object.entries(shape)) {
      part.setAttribute(key, value);
    }
    svg.append(part);
  }
  return svg;
}

/** the balloon's tail: a filled triangle with an outlined edge, pointing up. */
export function tail() {
  const svg = document.createElementNS(NAMESPACE, "svg");
  for (const [key, value] of Object.entries({
    class: "tail",
    "aria-hidden": "true",
    width: "16",
    height: "9",
    viewBox: "0 0 16 9",
  })) {
    svg.setAttribute(key, value);
  }
  for (const d of ["M0 9 L8 0 L16 9", "M0 8.5 L8 0.5 L16 8.5"]) {
    const path = document.createElementNS(NAMESPACE, "path");
    path.setAttribute("d", d);
    svg.append(path);
  }
  return svg;
}
