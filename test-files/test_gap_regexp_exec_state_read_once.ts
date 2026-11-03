// RegExpBuiltinExec reads the receiver's matcher data and lastIndex once, from
// the birth layout's own slots. Every observable consequence of the old
// separate reads must hold: a lastIndex whose ToLength runs user code and
// recompiles the receiver, a non-Number lastIndex, non-writable
// lastIndex, subclasses, an own exec override, Symbol.replace/match overrides,
// sticky/global state, and plain (birth layout) receivers beside all of them.
const out: string[] = [];
const log = (label: string, v: unknown): void => {
  out.push(label + "=" + JSON.stringify(v));
};

// 1. Plain receivers: non-global lastIndex is ignored and never written.
const plain = /(\d+)-(\w+)/;
plain.lastIndex = 5;
const m1 = plain.exec("ab 12-xy cd");
log("plain", m1 === null ? null : [m1[0], m1[1], m1[2], m1.index]);
log("plain-lastIndex", plain.lastIndex);

// 2. Global and sticky advance and reset lastIndex.
const g = /a(b)?/g;
const seq: unknown[] = [];
for (let i = 0; i < 4; i++) {
  const m = g.exec("abxab");
  seq.push(m === null ? null : [m[0], m[1], m.index, g.lastIndex]);
}
log("global", seq);
const y = /b/y;
y.lastIndex = 1;
log("sticky-hit", [y.exec("abc") !== null, y.lastIndex]);
log("sticky-miss", [y.exec("abc") !== null, y.lastIndex]);
const past = /a/g;
past.lastIndex = 99;
log("past-end", [past.exec("a") === null, past.lastIndex]);

// 3. lastIndex that is not a Number: ToLength runs valueOf, which may recompile
// the receiver; the search must use the data published by then.
const re = /aaa/g;
let calls = 0;
(re as any).lastIndex = {
  valueOf() {
    calls++;
    re.compile("b+", "g");
    return 0;
  },
};
const m3 = re.exec("aaabbb");
log("recompiled-in-valueOf", [m3 === null ? null : m3[0], calls, re.source, re.lastIndex]);

// Same through a sticky receiver whose lastIndex is a string.
const s2 = /x/y;
(s2 as any).lastIndex = "2";
log("string-lastIndex", [s2.exec("abxd") === null, s2.lastIndex]);
const s3 = /x/y;
(s3 as any).lastIndex = "2";
log("string-lastIndex-hit", [s3.exec("abxd")?.[0], s3.lastIndex]);

// valueOf that throws leaves the receiver untouched.
const thrower = /a/g;
(thrower as any).lastIndex = {
  valueOf() {
    throw new Error("boom");
  },
};
let caught = "";
try {
  thrower.exec("a");
} catch (e) {
  caught = (e as Error).message;
}
log("valueOf-throws", caught);

// 4. Non-writable lastIndex: a global exec that must write it throws.
const frozen = /a/g;
Object.defineProperty(frozen, "lastIndex", { writable: false, value: 0 });
let frozenError = "";
try {
  frozen.exec("a");
} catch (e) {
  frozenError = (e as Error).constructor.name;
}
log("non-writable-global", frozenError);
const frozenPlain = /a/;
Object.defineProperty(frozenPlain, "lastIndex", { writable: false, value: 0 });
log("non-writable-nonglobal", frozenPlain.exec("a")?.[0]);

// 5. Own property added to the receiver, subclass, own exec override.
const extra = /(o)k/;
(extra as any).tag = 1;
log("extra-own-key", extra.exec("ok")?.[1]);
class Sub extends RegExp {
  seen = 0;
  exec(s: string) {
    this.seen++;
    return super.exec(s);
  }
}
const sub = new Sub("(s)u", "g");
log("subclass", [sub.exec("su")?.[1], sub.seen, sub.lastIndex]);
const own = /z/;
let ownCalls = 0;
(own as any).exec = function (s: string) {
  ownCalls++;
  return /q/.exec(s);
};
log("own-exec", [own.exec("q")?.[0], ownCalls, own.test("q"), ownCalls]);

// 6. Symbol.replace / Symbol.match overrides and prototype exec patching.
const rep = /r/g;
(rep as any)[Symbol.replace] = () => "replaced";
log("symbol-replace", "arr".replace(rep, "-"));
const mat = /m/;
(mat as any)[Symbol.match] = () => ["matched"];
log("symbol-match", "m".match(mat));
const origExec = RegExp.prototype.exec;
let patched = 0;
RegExp.prototype.exec = function (this: RegExp, s: string) {
  patched++;
  return origExec.call(this, s);
};
const viaTest = /t/;
log("patched-proto-test", [viaTest.test("t"), patched > 0]);
const patchedExec = /(p)/.exec("p");
log("patched-proto-exec", [patchedExec?.[1], patched > 1]);
RegExp.prototype.exec = origExec;

// 7. Lead-unit exclusion and hasIndices.
const lead = /xyz+/g;
lead.lastIndex = 3;
log("lead-excluded", [lead.exec("abc") === null, lead.lastIndex]);
const di = /(b)/d;
const m8 = di.exec("abc");
log("indices", m8 === null ? null : (m8 as any).indices[1]);

// 8. Many executions on one receiver alongside GC-triggering allocation.
const many = /^(\w+)=(\d+)$/;
let sum = 0;
for (let i = 0; i < 20000; i++) {
  const m = many.exec("k" + (i % 10) + "=" + i);
  if (m !== null) sum += parseInt(m[2], 10);
}
log("many", sum);

console.log(out.join("\n"));
