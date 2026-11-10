// Sloppy CommonJS: mapped arguments share parameter cells, including arrows.
// Compare byte-for-byte with the pinned Node oracle.
function poke(args, value) { args[0] = value; return 1; }
function body(a, b) {
  arguments[0] = b;
  console.log("body", a === b, arguments[0] === b);
  a = 17;
  console.log("reverse", arguments[0], a);
}
body(1, 2);
function arrow(a, b) {
  a.n += 1;
  const write = () => { arguments[0] = b; };
  write();
  a.n += 1000;
  return a === b;
}
{
  const x = { n: 1 }, y = { n: 50 };
  console.log("arrow", arrow(x, y), x.n, y.n);
}
function nested(a, b) {
  const read = () => a;
  const write = () => () => { arguments[0] = b; };
  write()();
  console.log("nested", a === b, read() === b, arguments[0] === b);
  a = 29;
  console.log("nested-reverse", (() => arguments[0])(), read());
}
nested(1, 2);
function escaped(a, b) {
  const read = () => a;
  poke(arguments, b);
  console.log("helper", a === b, read() === b);
  a = 31;
  const args = arguments;
  console.log("helper-reverse", ((v) => v[0])(args));
}
escaped(1, 2);
// Compound assignment must preserve the receiver from before RHS/key writes.
function rhs(a, b) {
  a.n += (() => { arguments[0] = b; return 1; })();
  a.n += 1000;
  return a === b;
}
function key(a, b) {
  const write = () => { arguments[0] = b; return "n"; };
  a[write()] *= 10;
  return a === b;
}
function passed(a, b) {
  for (let i = 0; i < 2; i++) a[i] += poke(arguments, b);
  return a === b;
}
{
  const x = { n: 1 }, y = { n: 50 };
  console.log("rhs", rhs(x, y), x.n, y.n);
}
{
  const x = { n: 2 }, y = { n: 7 };
  console.log("key", key(x, y), x.n, y.n);
}
{
  const x = [1, 2], y = [10, 20];
  console.log("passed", passed(x, y), x.join(","), y.join(","));
}
function strict(a, b) {
  "use strict";
  (() => { arguments[0] = b; })();
  console.log("strict", a, arguments[0]);
  a = 41;
  console.log("strict-reverse", arguments[0]);
}
strict(1, 2);
function defaulted(a, b = 2) {
  (() => { arguments[0] = b; })();
  console.log("default", a, arguments[0], arguments.length);
  a = 43;
  console.log("default-reverse", arguments[0]);
}
defaulted(1);
function rest(a, ...b) {
  (() => { arguments[0] = b[0]; })();
  console.log("rest", a, arguments[0]);
  a = 47;
  console.log("rest-reverse", arguments[0]);
}
rest(1, 2);
function destructured(a, { value }) {
  (() => { arguments[0] = value; })();
  console.log("destructured", a, arguments[0]);
  a = 53;
  console.log("destructured-reverse", arguments[0]);
}
destructured(1, { value: 2 });
function missing(a, b) {
  arguments.length = 2; // Changing length cannot create mappings.
  (() => { arguments[1] = 59; })();
  console.log("missing", b === undefined, arguments[1]);
  b = 61;
  console.log("missing-reverse", arguments[1]);
  arguments[0] = 67;
  console.log("present", a);
}
missing(1);
function noArgs(a) {
  (() => { arguments[0] = 71; })();
  console.log("no-args", a === undefined, arguments[0]);
  a = 73;
  console.log("no-args-reverse", arguments[0]);
}
noArgs();
function deleted(a, b) {
  delete arguments[0];
  (() => { arguments[0] = b; })();
  console.log("deleted", a, arguments[0]);
  a = 79;
  console.log("deleted-reverse", arguments[0]);
}
deleted(1, 2);
function shrunk(a) {
  arguments.length = 0; // Supplied indices keep their mapping.
  arguments[0] = 83;
  console.log("shrunk", a, arguments.length);
}
shrunk(1);
const expression = function(a, b) {
  const read = () => a;
  (() => () => { arguments[0] = b; })()();
  return [a === b, read() === b, arguments[0] === b].join(",");
};
console.log("expression", expression(1, 2));
const holder = {
  method(a, b) {
    (() => { arguments[0] = b; })();
    return a === b;
  }
};
console.log("method", holder.method(1, 2));
// Both directions still work after the enclosing call returns.
function keep(a) {
  return {
    write: (v) => { arguments[0] = v; },
    read: () => a,
    set: (v) => { a = v; },
    arg: () => arguments[0]
  };
}
const saved = keep(1);
saved.write(89);
console.log("returned", saved.read(), saved.arg());
saved.set(97);
console.log("returned-reverse", saved.read(), saved.arg());

function every(a, b, c) {
  const write = () => {
    arguments[0] = b;
    arguments[1] = c;
    arguments[2] = 101;
  };
  write();
  console.log("every", a, b, c, arguments[0], arguments[1], arguments[2]);
}
every(1, 2, 3);
function duplicate(a, a) {
  const write = () => { arguments[0] = 103; };
  write();
  console.log("duplicate-first", a, arguments[0], arguments[1]);
  arguments[1] = 107;
  console.log("duplicate-last", a, arguments[0], arguments[1]);
  a = 109;
  console.log("duplicate-reverse", arguments[0], arguments[1]);
}
duplicate(1, 2);
