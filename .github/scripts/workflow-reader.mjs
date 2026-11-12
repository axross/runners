// reads the subset of YAML that this repository's workflow files use, with
// nothing but Node, because Node has no YAML parser and the project pins no
// other tool that has one. it relies on the indentation Prettier gives the
// files, and throws on anything outside that subset instead of guessing, so a
// check built on it cannot pass because it misread a file.
//
// supported: block mappings and sequences, plain, single-quoted and
// double-quoted scalars on one line (a plain one may continue on more indented
// lines), `|` and `>` block scalars with the `-` and `+` chomping marks, and
// flow sequences of scalars such as `[plan, build]`. not supported: anchors,
// tags, flow mappings other than `{}`, multi-document files, and explicit keys.
import { readFileSync } from "node:fs";

class ReadError extends Error {}

function fail(file, number, message) {
  throw new ReadError(`${file}:${number}: ${message}`);
}

// the position of the first ` #` that starts a comment, outside quotes.
function commentStart(text) {
  let quote = null;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (quote) {
      if (char === quote) quote = null;
    } else if (char === '"' || char === "'") {
      if (index === 0 || /[\s[{,:]/.test(text[index - 1])) quote = char;
    } else if (char === "#" && (index === 0 || /\s/.test(text[index - 1]))) {
      return index;
    }
  }
  return -1;
}

function stripComment(text) {
  const start = commentStart(text);
  return (start === -1 ? text : text.slice(0, start)).trimEnd();
}

function unquote(file, number, text) {
  if (text.startsWith('"')) {
    if (!text.endsWith('"') || text.length < 2)
      fail(file, number, `unterminated string ${text}`);
    return JSON.parse(text);
  }
  if (text.startsWith("'")) {
    if (!text.endsWith("'") || text.length < 2)
      fail(file, number, `unterminated string ${text}`);
    return text.slice(1, -1).replaceAll("''", "'");
  }
  return text;
}

function plainScalar(file, number, text) {
  if (text === "~" || text === "null") return null;
  if (text === "{}") return {};
  if (text === "[]") return [];
  if (text.startsWith("[")) {
    if (!text.endsWith("]"))
      fail(file, number, `a flow sequence must close on its line: ${text}`);
    const inner = text.slice(1, -1).trim();
    if (inner === "") return [];
    if (/[[\]{}]/.test(inner))
      fail(file, number, `a nested flow collection is not supported: ${text}`);
    return inner.split(",").map((item) => unquote(file, number, item.trim()));
  }
  if (text.startsWith("{"))
    fail(file, number, `a flow mapping is not supported: ${text}`);
  if (/^[&*!]/.test(text))
    fail(file, number, `an anchor, alias or tag is not supported: ${text}`);
  return unquote(file, number, text);
}

const KEY = /^("[^"]*"|'[^']*'|[A-Za-z0-9_./*-]+)[ \t]*:(?:[ \t]+(.*))?$/;

// parses `text`, the contents of `file`, into plain objects, arrays and strings.
// a mapping value that is a block scalar keeps its text.
export function parseYaml(text, file = "<text>") {
  const lines = text.split(/\r?\n/).map((raw, index) => {
    const content = raw.trimStart();
    return {
      number: index + 1,
      raw,
      indent: raw.length - content.length,
      content,
    };
  });
  const blank = (line) => line.content === "" || line.content.startsWith("#");

  function nextReal(from) {
    let index = from;
    while (index < lines.length && blank(lines[index])) index += 1;
    return index;
  }

  // reads the block scalar whose header is on `lines[headerIndex]`; its content
  // is every following line more indented than `parentIndent`, or blank.
  function blockScalar(headerIndex, header, parentIndent) {
    const style = header[0];
    const chomp = header.slice(1);
    if (!/^[-+]?$/.test(chomp))
      fail(
        file,
        lines[headerIndex].number,
        `unsupported block scalar header ${header}`,
      );
    const body = [];
    let index = headerIndex + 1;
    let contentIndent = null;
    while (index < lines.length) {
      const line = lines[index];
      if (line.content === "") {
        body.push({ blank: true });
      } else if (
        line.indent > parentIndent &&
        (contentIndent === null || line.indent >= contentIndent)
      ) {
        if (contentIndent === null) contentIndent = line.indent;
        body.push({ text: line.raw.slice(contentIndent) });
      } else {
        break;
      }
      index += 1;
    }
    let end = body.length;
    while (end > 0 && body[end - 1].blank) end -= 1;
    const kept = body
      .slice(0, end)
      .map((entry) => (entry.blank ? "" : entry.text));
    let value;
    if (style === "|") {
      value = kept.join("\n");
    } else {
      value = kept.join("\n").replace(/([^\n])\n(?=[^\n\s])/g, "$1 ");
    }
    if (kept.length > 0 && chomp !== "-") value += "\n";
    if (chomp === "+") value += "\n".repeat(body.length - end);
    return [value, index];
  }

  // a plain scalar may continue on following lines that are more indented than
  // the key it belongs to.
  function scalarWithContinuation(index, valueText, parentIndent) {
    let value = valueText;
    let next = index + 1;
    for (;;) {
      const afterBlank = nextReal(next);
      if (
        afterBlank >= lines.length ||
        lines[afterBlank].indent <= parentIndent
      )
        break;
      if (
        value.startsWith('"') ||
        value.startsWith("'") ||
        value.startsWith("[")
      ) {
        fail(
          file,
          lines[afterBlank].number,
          "a quoted or flow value continues on another line",
        );
      }
      value += ` ${stripComment(lines[afterBlank].content)}`;
      next = afterBlank + 1;
    }
    return [plainScalar(file, lines[index].number, value), next];
  }

  function parseNode(start, indent) {
    const index = nextReal(start);
    if (index >= lines.length) return [null, index];
    const line = lines[index];
    if (line.indent !== indent)
      fail(
        file,
        line.number,
        `expected indentation ${indent}, found ${line.indent}`,
      );
    if (line.content === "-" || line.content.startsWith("- "))
      return parseSequence(index, indent);
    if (KEY.test(stripComment(line.content)))
      return parseMapping(index, indent);
    return scalarWithContinuation(
      index,
      stripComment(line.content),
      indent - 1,
    );
  }

  function parseSequence(start, indent) {
    const items = [];
    let index = start;
    for (;;) {
      index = nextReal(index);
      if (index >= lines.length) break;
      const line = lines[index];
      if (line.indent < indent) break;
      if (line.indent > indent)
        fail(
          file,
          line.number,
          `unexpected indentation ${line.indent} in a sequence at ${indent}`,
        );
      if (!(line.content === "-" || line.content.startsWith("- "))) break;
      const rest = line.content === "-" ? "" : line.content.slice(2);
      const restTrim = rest.trimStart();
      if (restTrim === "" || restTrim.startsWith("#")) {
        const childIndex = nextReal(index + 1);
        if (childIndex >= lines.length || lines[childIndex].indent <= indent) {
          items.push(null);
          index += 1;
        } else {
          const [value, next] = parseNode(index + 1, lines[childIndex].indent);
          items.push(value);
          index = next;
        }
        continue;
      }
      // the item's own content sits further right on the dash's line: parse
      // it as though it were a line of its own at that column.
      const column = line.indent + (line.content.length - restTrim.length);
      lines[index] = {
        number: line.number,
        raw: " ".repeat(column) + restTrim,
        indent: column,
        content: restTrim,
      };
      const [value, next] = parseNode(index, column);
      items.push(value);
      index = next;
    }
    return [items, index];
  }

  function parseMapping(start, indent) {
    const map = Object.create(null);
    let index = start;
    for (;;) {
      index = nextReal(index);
      if (index >= lines.length) break;
      const line = lines[index];
      if (line.indent < indent) break;
      if (line.indent > indent)
        fail(
          file,
          line.number,
          `unexpected indentation ${line.indent} in a mapping at ${indent}`,
        );
      if (line.content === "-" || line.content.startsWith("- ")) break;
      const match = KEY.exec(stripComment(line.content));
      if (!match)
        fail(
          file,
          line.number,
          `cannot read this line as a key: ${line.content}`,
        );
      const key = unquote(file, line.number, match[1]);
      if (key in map) fail(file, line.number, `duplicate key ${key}`);
      const rest = (match[2] ?? "").trim();
      if (rest === "") {
        const childIndex = nextReal(index + 1);
        if (childIndex >= lines.length) {
          map[key] = null;
          index += 1;
        } else if (lines[childIndex].indent > indent) {
          const [value, next] = parseNode(index + 1, lines[childIndex].indent);
          map[key] = value;
          index = next;
        } else if (
          lines[childIndex].indent === indent &&
          lines[childIndex].content.startsWith("- ")
        ) {
          fail(
            file,
            lines[childIndex].number,
            "a sequence at the indentation of its key is not supported",
          );
        } else {
          map[key] = null;
          index += 1;
        }
      } else if (/^[|>]/.test(rest)) {
        const [value, next] = blockScalar(index, rest, indent);
        map[key] = value;
        index = next;
      } else {
        const [value, next] = scalarWithContinuation(index, rest, indent);
        map[key] = value;
        index = next;
      }
    }
    return [map, index];
  }

  const first = nextReal(0);
  if (first >= lines.length) return null;
  const [value, end] = parseNode(first, lines[first].indent);
  const rest = nextReal(end);
  if (rest < lines.length)
    fail(
      file,
      lines[rest].number,
      `unexpected content: ${lines[rest].content}`,
    );
  return value;
}

export function readWorkflow(path) {
  return parseYaml(readFileSync(path, "utf8"), path);
}

// the scalar text of a `${{ ... }}`-bearing or plain value, for callers that
// treat a missing key as the empty string.
export function text(value) {
  return typeof value === "string" ? value : "";
}

// the entries of a `needs` value as an array of job names.
export function asList(value) {
  if (value === null || value === undefined) return [];
  return Array.isArray(value) ? value : [value];
}
