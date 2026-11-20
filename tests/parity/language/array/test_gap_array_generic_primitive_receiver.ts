// Array.prototype generic methods on primitive receivers: the receiver goes
// through ToObject, and the generic algorithm reads length and indexed
// elements off the wrapper (String wrappers expose their code units, Number
// and Boolean wrappers have length 0). Each method is reached two ways: as
// `Array.prototype.m.call(prim, ...)` and through a local holding the method.

function show(label: string, value: any): void {
  if (Array.isArray(value)) {
    console.log(label + ": " + JSON.stringify(value) + " length=" + value.length);
  } else {
    console.log(label + ": " + typeof value + " " + String(value));
  }
}

function attempt(label: string, fn: () => any): void {
  try {
    show(label, fn());
  } catch (e: any) {
    console.log(label + ": throws " + e.constructor.name);
  }
}

const receivers: [string, any][] = [
  ["string", "abc"],
  ["empty string", ""],
  ["number", 42],
  ["boolean", true],
];

const slice = Array.prototype.slice;
const map = Array.prototype.map;
const indexOf = Array.prototype.indexOf;
const join = Array.prototype.join;
const forEach = Array.prototype.forEach;
const includes = Array.prototype.includes;

for (const [name, r] of receivers) {
  attempt("slice.call " + name, () => Array.prototype.slice.call(r, 1));
  attempt("slice local " + name, () => slice.call(r));
  attempt("map.call " + name, () =>
    Array.prototype.map.call(r, (v: any, i: number, o: any) => i + ":" + v + ":" + typeof o));
  attempt("map local " + name, () => map.call(r, (v: any) => String(v).toUpperCase()));
  attempt("indexOf.call " + name, () => Array.prototype.indexOf.call(r, "b"));
  attempt("indexOf local " + name, () => indexOf.call(r, "c", 1));
  attempt("join.call " + name, () => Array.prototype.join.call(r, "-"));
  attempt("join local " + name, () => join.call(r));
  attempt("forEach.call " + name, () => {
    const seen: string[] = [];
    Array.prototype.forEach.call(r, (v: any, i: number, o: any) => {
      seen.push(i + "=" + v + "/" + typeof o);
    });
    return seen;
  });
  attempt("forEach local " + name, () => {
    const seen: string[] = [];
    forEach.call(r, (v: any) => { seen.push(String(v)); });
    return seen;
  });
  attempt("includes.call " + name, () => Array.prototype.includes.call(r, "c"));
  attempt("includes local " + name, () => includes.call(r, "a"));
}

// Mutating methods: a String wrapper's indices and length are read-only, so
// they throw; Number and Boolean wrappers are ordinary extensible objects.
attempt("push.call string", () => Array.prototype.push.call("abc", "d"));
attempt("pop.call string", () => Array.prototype.pop.call("abc"));
attempt("reverse.call string", () => Array.prototype.reverse.call("abc"));
attempt("push.call boolean", () => Array.prototype.push.call(true, 1));
attempt("reverse.call boolean", () => typeof Array.prototype.reverse.call(false));
attempt("keys.call string", () => Array.from(Array.prototype.keys.call("ab")));
attempt("values.call string", () => Array.from(Array.prototype.values.call("ab")));
attempt("entries.call string", () => Array.from(Array.prototype.entries.call("ab")));

// The same calls through apply, and the null / undefined receivers.
attempt("slice.apply string", () => Array.prototype.slice.apply("xyz", [0, 2]));
attempt("join.apply number", () => Array.prototype.join.apply(7, ["+"]));
attempt("slice.call null", () => Array.prototype.slice.call(null as any));
attempt("forEach.call undefined", () => Array.prototype.forEach.call(undefined as any, () => {}));
