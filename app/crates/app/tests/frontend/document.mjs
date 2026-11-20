// a small stand-in for the browser's document, enough for the page's element builder
// and for the page itself to draw into, since no browser runs under Node. It supports
// what the page's code calls and nothing more.

export class Element {
  constructor(tag) {
    this.tag = tag;
    this.attributes = new Map();
    this.listeners = new Map();
    this.children = [];
    this.parent = null;
    this.value = "";
    this.textContent = "";
    this.hidden = false;
  }
  get dataset() {
    const data = {};
    for (const [name, value] of this.attributes) {
      if (name.startsWith("data-")) {
        data[name.slice(5)] = value;
      }
    }
    return data;
  }
  get firstElementChild() {
    return this.children.find((child) => child instanceof Element) ?? null;
  }
  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }
  getAttribute(name) {
    return this.attributes.get(name) ?? null;
  }
  removeAttribute(name) {
    this.attributes.delete(name);
  }
  addEventListener(type, listener) {
    const held = this.listeners.get(type) ?? [];
    held.push(listener);
    this.listeners.set(type, held);
  }
  dispatchEvent(event) {
    for (const listener of this.listeners.get(event.type) ?? []) {
      listener(event);
    }
    return true;
  }
  append(...nodes) {
    for (const node of nodes) {
      if (node instanceof Element) {
        node.parent = this;
      }
    }
    this.children.push(...nodes);
  }
  get classList() {
    const names = () => this.getAttribute("class")?.split(" ") ?? [];
    return { contains: (name) => names().includes(name) };
  }
  /** this element or the nearest ancestor matching one of `.name` or `[name]`, as a list. */
  closest(selector) {
    const matches = (element) =>
      selector.split(",").some((part) => {
        const simple = part.trim();
        return simple.startsWith(".")
          ? element.classList.contains(simple.slice(1))
          : element.attributes.has(simple.slice(1, -1));
      });
    for (let element = this; element; element = element.parent) {
      if (matches(element)) return element;
    }
    return null;
  }
  /** the first descendant matching the simple class or tag selectors the cards use. */
  querySelector(selector) {
    return (
      [...this.descendants()].find(
        (element) =>
          element !== this &&
          (selector.startsWith(".")
            ? element
                .getAttribute("class")
                ?.split(" ")
                .includes(selector.slice(1))
            : element.tag === selector),
      ) ?? null
    );
  }
  replaceChildren(...nodes) {
    this.children = [];
    this.append(...nodes);
  }
  remove() {
    if (this.parent) {
      this.parent.children = this.parent.children.filter(
        (child) => child !== this,
      );
    }
  }
  focus() {
    globalThis.document.activeElement = this;
  }
  contains(node) {
    return (
      node === this || this.children.some((child) => child.contains?.(node))
    );
  }
  /** the descendants, this element first, in document order. */
  *descendants() {
    yield this;
    for (const child of this.children) {
      if (child instanceof Element) {
        yield* child.descendants();
      }
    }
  }
  /** the elements `[name]` or `[name=value]` picks, the only selectors the page uses. */
  querySelectorAll(selector) {
    const [, name, value] = /^\[([\w-]+)(?:=([\w-]+))?\]$/.exec(selector);
    return [...this.descendants()].filter(
      (element) =>
        element !== this &&
        element.attributes.has(name) &&
        (value === undefined || element.attributes.get(name) === value),
    );
  }
  find(tag) {
    for (const child of this.children) {
      if (child.tag === tag) {
        return child;
      }
      const inside = child.find?.(tag);
      if (inside) {
        return inside;
      }
    }
    return null;
  }
}

/** installs the stand-in as the global `document`, with the elements the page looks up. */
export function installDocument(ids = []) {
  const byId = new Map(ids.map((id) => [id, new Element("div")]));
  const listeners = new Map();
  globalThis.document = {
    hidden: false,
    activeElement: null,
    createElement: (tag) => new Element(tag),
    createElementNS: (_namespace, tag) => new Element(tag),
    createTextNode: (text) => ({ text }),
    getElementById: (id) => byId.get(id) ?? null,
    addEventListener: (type, listener) => {
      listeners.set(type, [...(listeners.get(type) ?? []), listener]);
    },
    dispatchEvent: (event) => {
      for (const listener of listeners.get(event.type) ?? []) {
        listener(event);
      }
    },
  };
  return byId;
}
