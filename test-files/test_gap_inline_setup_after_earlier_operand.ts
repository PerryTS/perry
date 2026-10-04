// An inlined call's parameter bindings were spliced in front of the whole
// statement, ahead of the operands evaluated before the call. A later
// argument that read a local assigned by an earlier argument saw the old
// value: TypeScript's conditional-expression parser does
//   make(cond, q, whenTrue, colon = parseExpected(COLON), present(colon) ? ... : missing)
// and prettier's typescript plugin threw "':' expected" on every `a ? b : c`.
// Every line must match node.

function missing(e) {
  return e === void 0 ? !0 : e.pos === e.end && e.pos >= 0 && e.kind !== 1;
}
function present(e) {
  return !missing(e);
}
function token() {
  return { pos: 15, end: 17, kind: 59 };
}
const factory = {
  make(a, b, c, d, e) {
    return [a, b, c, d && d.kind, e].join(",");
  },
};
function opaque(a, b) {
  for (let i = 0; i < 1; i++) {}
  return [a && a.kind, b].join(",");
}

// The parser's shape: a member call, a store, then a read of the stored local.
function conditional(m) {
  for (let i = 0; i < 1; i++) {}
  let colon;
  return factory.make(1, 2, (() => 3)(), (colon = token()), present(colon) ? m : "MISSING");
}
console.log("member call", conditional("whenFalse"));

// A plain call to a function the inliner keeps.
function plain() {
  for (let i = 0; i < 1; i++) {}
  let x;
  return opaque((x = token()), present(x));
}
console.log("plain call", plain());

// Effects in an earlier argument run before a later argument's inlined call.
const log = [];
function note(tag) {
  log.push(tag);
  return { pos: 1, end: 2, kind: 3 };
}
function order() {
  for (let i = 0; i < 1; i++) {}
  log.length = 0;
  const r = opaque(note("first"), present(note("second")));
  return r + " " + log.join(">");
}
console.log("effects", order());

// Other operand positions: binary, array, object, property store, comma.
function positions() {
  for (let i = 0; i < 1; i++) {}
  let a;
  const sum = ((a = token()), 0) + (present(a) ? 1 : 0);
  let b;
  const arr = [(b = token()), present(b)];
  let c;
  const obj = { first: (c = token()), second: present(c) };
  let d;
  const holder = { v: false };
  holder[((d = token()), "v")] = present(d);
  let e;
  const seq = ((e = token()), present(e));
  return [sum, arr[1], obj.second, holder.v, seq].join(",");
}
console.log("positions", positions());

// Reads before the call stay inlinable and correct.
function reads(y) {
  for (let i = 0; i < 1; i++) {}
  const x = token();
  return opaque(y, present(x));
}
console.log("reads", reads(token()));
