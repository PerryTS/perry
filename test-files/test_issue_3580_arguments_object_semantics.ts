// Issue #3580: ECMAScript arguments objects.
// This fixture is intentionally byte-for-byte comparable with Node.

function descShape(desc) {
  return [
    typeof desc.value,
    desc.writable,
    desc.enumerable,
    desc.configurable,
    typeof desc.get,
    typeof desc.set,
  ].join("|");
}

function observeThrow(label, read) {
  try {
    read();
    console.log(label + ": no throw");
  } catch (_err) {
    console.log(label + ": TypeError");
  }
}

function basics(a, b) {
  console.log("basic class:", Array.isArray(arguments), Object.prototype.toString.call(arguments));
  console.log("basic keys:", Object.keys(arguments).join(","));
  console.log("basic indexed:", [arguments.length, arguments[0], arguments[1], arguments[2]].join("|"));
  console.log("desc 0:", descShape(Object.getOwnPropertyDescriptor(arguments, "0")));
  console.log("desc length:", descShape(Object.getOwnPropertyDescriptor(arguments, "length")));
}
basics("x", "y");

const sloppyCalleeDescriptor = new Function(`
  const desc = Object.getOwnPropertyDescriptor(arguments, "callee");
  return [
    typeof desc.value,
    desc.writable,
    desc.enumerable,
    desc.configurable,
    typeof desc.get,
    typeof desc.set,
  ].join("|");
`);
console.log("desc callee sloppy:", sloppyCalleeDescriptor());

const mappedAlias = new Function("a", `
  console.log("mapped 0:", [a, arguments[0]].join("|"));
  arguments[0] = 7;
  console.log("mapped 1:", [a, arguments[0]].join("|"));
  a = 9;
  console.log("mapped 2:", [a, arguments[0]].join("|"));
  Object.defineProperty(arguments, "0", { value: 11 });
  console.log("mapped 3:", [a, arguments[0]].join("|"));
  Object.defineProperty(arguments, "0", { writable: false });
  a = 13;
  console.log("mapped 4:", [a, arguments[0], Object.getOwnPropertyDescriptor(arguments, "0").writable].join("|"));
`);
mappedAlias(1);

const deleteBreaksMapping = new Function("a", `
  delete arguments[0];
  a = 5;
  console.log("mapped delete:", [a, arguments[0], Object.prototype.hasOwnProperty.call(arguments, "0")].join("|"));
`);
deleteBreaksMapping(1);

function strictUnmapped(a) {
  "use strict";
  arguments[0] = 7;
  const afterSet = a;
  a = 9;
  console.log("strict unmapped:", [afterSet, arguments[0]].join("|"));
  console.log("desc callee strict:", descShape(Object.getOwnPropertyDescriptor(arguments, "callee")));
  observeThrow("strict callee", () => arguments.callee);
}
strictUnmapped(1);

function defaultUnmapped(a = 1) {
  arguments[0] = 7;
  const afterSet = a;
  a = 9;
  console.log("default unmapped:", [afterSet, arguments[0]].join("|"));
  observeThrow("default callee", () => arguments.callee);
}
defaultUnmapped(1);

function restUnmapped(a, ...rest) {
  arguments[0] = 7;
  const afterSet = a;
  a = 9;
  console.log("rest unmapped:", [afterSet, arguments[0], rest.join("-")].join("|"));
  observeThrow("rest callee", () => arguments.callee);
}
restUnmapped(1, 2, 3);

function arrowInheritance(a) {
  "use strict";
  const inner = () => [arguments.length, arguments[0], a].join("|");
  arguments[0] = 8;
  return inner();
}
console.log("arrow inherits:", arrowInheritance(4, 5));

function outerApply() {
  return (function (x, y) {
    return x + ":" + y;
  }).apply(null, arguments);
}

function arrayLikeConsumers() {
  console.log("array like:", [
    Array.from(arguments).join("-"),
    [].slice.call(arguments, 1).join("-"),
    outerApply("a", "b"),
  ].join("|"));
}
arrayLikeConsumers("q", "r", "s");

function RestCtor(...rest) {
  console.log("rest new.target:", [
    new.target === RestCtor,
    arguments.length,
    arguments[0],
    rest.join("-"),
  ].join("|"));
}
new (RestCtor as any)("n", "m");
