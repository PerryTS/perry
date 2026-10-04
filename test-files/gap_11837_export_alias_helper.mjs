// #11837 helper: every export below is published under a name that a
// different local function of this module also has.

// prettier index.mjs: the wrapper is exported under the wrapped function name.
async function formatWithCursor(text, options) {
  return { formatted: text + options.suffix };
}
function withPlugins(fn) {
  return async (...args) => fn(...args);
}
var formatWithCursor2 = withPlugins(formatWithCursor);

// The same, synchronous.
function inner(text) {
  return "inner:" + text;
}
function wrap(fn) {
  return (t) => "wrapped(" + fn(t) + ")";
}
var inner2 = wrap(inner);

// A function exported under the name of another local function.
function first() {
  return "first";
}
function second() {
  return "second";
}

// A name that only differs in characters the symbol mangling rewrites.
function _i() {
  return "underscore";
}
const dollar = () => "dollar";

// A nested function with the name of a top-level export.
export const helper = () => "exported helper";
function outer() {
  function helper() {
    return "nested helper";
  }
  return helper();
}

export function localCalls() {
  return [inner("a"), second(), _i(), outer()].join(" ");
}

export {
  formatWithCursor2 as formatWithCursor,
  inner2 as inner,
  first as second,
  dollar as $i,
};
