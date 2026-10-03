// #11619: optional-chain call bases evaluate once, including async and fields.
const q = [() => console.log("called A"), () => console.log("called B")];
q.shift()?.();
console.log("left in queue:", q.length);
let n = 0;
const f = () => { n++; return { x: 1, nested: { y: 2 }, method() { return this.x; } }; };
console.log("value", f()?.x, "calls", n);
console.log("nested", f()?.nested?.y, "calls", n);
console.log("method", f()?.method(), "calls", n);
let k = 0;
const h = () => { k++; return undefined as undefined | { x: number }; };
let keys = 0;
console.log("nullish", h()?.[(keys++, "x")], "calls", k, "keys", keys);
class Field { value = f()?.x; static value = f()?.x; }
const a = new Field(); const b = new Field();
console.log("fields", a.value, b.value, Field.value, "calls", n);
function scope() { return f()?.x; }
console.log("function", scope(), "calls", n);
let depth = 0;
function recursiveBase(): { x: number } {
  depth++;
  if (depth < 3) { const inner = recursiveBase()?.x; console.log("inner", inner); }
  return { x: depth };
}
console.log("recursive", recursiveBase()?.x, "depth", depth);
let m = 0;
const g = async () => { m++; return { x: 1 }; };
console.log("async", (await g())?.x, "calls", m);
async function suspend() {
  const base = f()?.[(await Promise.resolve("x"))];
  return base;
}
console.log("suspend", await suspend(), "calls", n);

let ternaries = 0, branches = 0;
function choice() { ternaries++; return true; }
function branch() { branches++; return { x: 7, method() { return this.x; } }; }
console.log("ternary", (choice() ? branch() : null)?.x, ternaries, branches);
console.log("ternary method", (choice() ? branch() : null)?.method(), ternaries, branches);
let children = 0;
const holder = { get child() { children++; return { x: 9, method() { return this.x; } }; } };
console.log("getter base", holder.child?.x, children);
console.log("getter method base", holder.child?.method(), children);
let nestedCalls = 0;
function outer() { nestedCalls++; return { inner() { nestedCalls++; return { x: 11, method() { return this.x; } }; } }; }
console.log("nested calls", outer()?.inner()?.x, nestedCalls);
console.log("nested method calls", outer()?.inner()?.method(), nestedCalls);

let defaults = 0;
function defaultBase() { defaults++; return { x: 17 }; }
function defaulted(value = defaultBase()?.x) { return value; }
console.log("defaults", defaulted(), defaulted(23), defaulted(undefined), defaults);
class DefaultField { value: number; constructor(value = defaultBase()?.x) { this.value = value; } }
console.log("constructor default", new DefaultField().value, defaults);
