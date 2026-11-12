// builds elements from fixed structure and sets every text from the view as text: no
// markup is ever parsed, and an attribute that could load, run or style anything is
// refused.

const FORBIDDEN = /^(style|href|src|srcdoc|action|formaction|on.*)$/i;

/**
 * an element with `props` as attributes (`on` holds listeners, `text` the element's
 * text) and `children` appended; strings become text nodes.
 */
export function h(tag, props = {}, ...children) {
  const element = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (value === undefined || value === null || value === false) {
      continue;
    }
    if (key === "on") {
      for (const [type, listener] of Object.entries(value)) {
        element.addEventListener(type, listener);
      }
    } else if (key === "text") {
      element.textContent = value;
    } else if (FORBIDDEN.test(key)) {
      throw new Error(`the attribute ${key} is not set from the page`);
    } else {
      element.setAttribute(key, value === true ? "" : String(value));
    }
  }
  append(element, children);
  return element;
}

/** appends strings as text and nodes as they are; arrays nest, and empty values are skipped. */
export function append(parent, children) {
  for (const child of children) {
    if (Array.isArray(child)) {
      append(parent, child);
    } else if (typeof child === "string") {
      parent.append(document.createTextNode(child));
    } else if (child) {
      parent.append(child);
    }
  }
}
