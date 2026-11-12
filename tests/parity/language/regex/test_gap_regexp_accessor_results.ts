// What each RegExp.prototype accessor (ECMA-262 22.2.6) returns on every kind
// of receiver: `flags` and `source` a String, the eight flag getters a
// Boolean on a RegExp and `undefined` on %RegExp.prototype% itself, a
// TypeError on any other receiver; a redefined accessor returns what the
// program's getter returns; `flags` reads the flag properties through [[Get]]
// (program getters run) and still returns a String. The results are used as
// values (typeof, ===, truthiness, arithmetic, string concatenation) so a
// wrong declared result shows as a wrong answer here.

const FLAGS = [
  "hasIndices",
  "global",
  "ignoreCase",
  "multiline",
  "dotAll",
  "unicode",
  "unicodeSets",
  "sticky",
];
const ALL = ["flags", "source", ...FLAGS];

function use(v: any): string {
  const n = typeof v === "symbol" ? "sym" : String(v);
  return typeof v + ":" + n + ":" + (v ? "T" : "F") + ":" + (v === true) + ":" + (v === undefined) + ":" + (typeof v === "string" ? v.length : -1);
}

function getter(name: string): any {
  return (Object.getOwnPropertyDescriptor(RegExp.prototype, name) as any).get;
}

function viaCall(name: string, recv: any): string {
  try {
    return use(getter(name).call(recv));
  } catch (e: any) {
    return "throws " + (e instanceof TypeError ? "TypeError" : String(e));
  }
}

function viaLoad(name: string, recv: any): string {
  try {
    return use(recv[name]);
  } catch (e: any) {
    return "throws " + (e instanceof TypeError ? "TypeError" : String(e));
  }
}

// Direct reads with static keys (the solve sees the key).
const re: any = /a+/dgimsy;
console.log("re.flags", use(re.flags), "re.source", use(re.source));
console.log("re.global", use(re.global), "re.ignoreCase", use(re.ignoreCase), "re.multiline", use(re.multiline));
console.log("re.dotAll", use(re.dotAll), "re.unicode", use(re.unicode), "re.unicodeSets", use(re.unicodeSets));
console.log("re.sticky", use(re.sticky), "re.hasIndices", use(re.hasIndices), "re.lastIndex", use(re.lastIndex));
const rv: any = new RegExp("b", "v");
console.log("rv", use(rv.flags), use(rv.unicodeSets), use(rv.unicode), use(rv.global));
const typedRe = /c/u;
const g: boolean = typedRe.global;
const u: boolean = typedRe.unicode;
console.log("typed", g, u, typedRe.source + "!", typedRe.flags.length);

// RegExp.prototype as the receiver, by a direct read.
const P: any = RegExp.prototype;
console.log("P.flags", use(P.flags), "P.source", use(P.source));
console.log("P.global", use(P.global), "P.sticky", use(P.sticky), "P.hasIndices", use(P.hasIndices));
console.log("P.global + 1", P.global + 1, "P.source + 1", P.source + 1);

// Every accessor on every receiver kind, by the getter and by a computed load.
class Sub extends RegExp {}
class SubOwn extends RegExp {
  get global(): any {
    return 7;
  }
  get source(): any {
    return 42;
  }
}
const receivers: [string, any][] = [
  ["regexp", /x/gi],
  ["subclass", new Sub("y", "msy")],
  ["subclass-own", new SubOwn("z", "g")],
  ["prototype", RegExp.prototype],
  ["sub-prototype", Sub.prototype],
  ["proxy", new Proxy(/p/g, {})],
  ["plain", { global: 1, sticky: "", source: "s" }],
  ["array", [1]],
  ["function", function f() {}],
  ["undefined", undefined],
  ["null", null],
  ["string", "str"],
  ["number", 3],
];
for (const [label, r] of receivers) {
  for (const name of ALL) {
    console.log(label, name, "call", viaCall(name, r), "load", viaLoad(name, r));
  }
}

// `flags` reads the flag properties through [[Get]]: program getters run.
const seen: string[] = [];
const spy: any = {};
for (const name of FLAGS) {
  Object.defineProperty(spy, name, {
    get() {
      seen.push(name);
      return name === "global" || name === "sticky" ? { truthy: true } : 0;
    },
  });
}
const sf = getter("flags").call(spy);
console.log("spy flags", use(sf), seen.length, seen.slice().sort().join(","));

// An accessor redefined on one RegExp instance.
const own: any = /o/g;
Object.defineProperty(own, "global", { get() { return 1; }, configurable: true });
Object.defineProperty(own, "source", { get() { return { s: 1 }; }, configurable: true });
Object.defineProperty(own, "sticky", { value: "yes", configurable: true });
console.log("own.global", use(own.global), "own.global + 1", own.global + 1);
console.log("own.source", typeof own.source, JSON.stringify(own.source));
console.log("own.sticky", use(own.sticky), "own.flags", use(own.flags));
console.log("own via getter", viaCall("global", own), viaCall("source", own));

// Accessors redefined on RegExp.prototype itself, then restored.
const saved: any = {};
for (const name of ["global", "source", "flags"]) {
  saved[name] = Object.getOwnPropertyDescriptor(RegExp.prototype, name);
}
Object.defineProperty(RegExp.prototype, "global", { get() { return 2; }, configurable: true });
Object.defineProperty(RegExp.prototype, "source", { get() { return null; }, configurable: true });
const later: any = /later/i;
console.log("redefined global", use(later.global), later.global * 3);
console.log("redefined source", use(later.source));
console.log("flags after redefinition", use(later.flags), use(P.flags));
console.log("split after redefinition", JSON.stringify("a1b2c".split(/\d/)));
console.log("replace after redefinition", "a1b2c".replace(/\d/g, "-"));
Object.defineProperty(RegExp.prototype, "flags", { get() { return 5; }, configurable: true });
console.log("redefined flags", use(later.flags));
for (const name of ["global", "source", "flags"]) {
  Object.defineProperty(RegExp.prototype, name, saved[name]);
}
console.log("restored", use(later.global), use(later.source), use(later.flags));

// The RegExp protocol of match / replace / split / matchAll / replaceAll
// reads `flags`; a subclass getter shows what it reads.
class Logged extends RegExp {
  get flags(): string {
    seen.push("flags");
    return super.flags;
  }
}
seen.length = 0;
const lg = new Logged("[0-9]", "g");
console.log("match", JSON.stringify("a1b2".match(lg)));
console.log("replace", "a1b2".replace(lg, "#"));
console.log("split", JSON.stringify("a1b2".split(lg)));
console.log("replaceAll", "a1b2".replaceAll(lg, "_"));
console.log("matchAll", JSON.stringify([..."a1b2".matchAll(lg)].map((m) => m[0])));
console.log("flags reads", seen.length > 0, seen.every((s) => s === "flags"));
console.log("whitespace split", JSON.stringify(" r  n b ".trim().split(/\s+/)));
