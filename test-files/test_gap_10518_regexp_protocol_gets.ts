// #10518: String.prototype.replace/replaceAll/match/split and the `flags`
// getter skip their RegExp protocol Gets (`flags` and its eight flag Gets,
// `@@replace`/`@@match`/`@@split`, `constructor` and `@@species`) when the
// RegExp is untouched and every one of them would reach a builtin. Everything
// below checks that the fast answers are right, and that each thing that makes
// one of those Gets observable is still observed.

const show = (v: any) => JSON.stringify(v);

// --- Untouched RegExps: the fast answers themselves.
const flagSets = ["", "g", "i", "m", "s", "u", "y", "d", "gi", "gimsuyd", "gv", "dgimsvy"];
// Receivers are `any` throughout: a local typed `RegExp` has its `.flags`
// folded at compile time, which is not the getter this exercises.
for (const f of flagSets) {
  const re: any = new RegExp("a", f);
  console.log("flags", show(f), "->", show(re.flags));
}
const lit1: any = /x/gi, lit2: any = /x/yimg;
console.log("literal flags", lit1.flags, lit2.flags);

const s = "a+b+c";
console.log(s.replace(/\+/g, " "), s.replace(/\+/, " "), s.replace(/\+/g, ""));
console.log(s.replace(/(\w)\+/g, "$1-"), s.replace(/\w/g, (m, i) => m + i));
console.log(s.replaceAll(/\+/g, "_"), s.replaceAll(/(?<l>\w)/g, "[$<l>]"));
console.log(show(s.match(/\+/g)), show(s.match(/\+/)), show(s.match(/z/g)), show(s.match(/z/)));
console.log(show("aaa".match(/a*?/g)), show("😀😀".match(/(?:)/gu)));
console.log(show(s.split(/\+/)), show(s.split(/\+/, 2)), show(s.split(/\+/, 0)));
console.log(show(s.split(/(\+)/)), show(s.split(/x*/)), show("".split(/x/)), show("".split(/x*/)));
console.log(show("😀😀".split(/(?:)/u)), show("a,b".split(/,/y)), show("a,b".split(/,/g)));
console.log(show("A1b2C".split(/\d/i)));
const b64 = "ndKK8/0z8Qw+OyczXXftx3rfjSEDizNo8Sb5awzd7Fw=";
console.log(b64.replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_"));

// lastIndex effects of the fast paths match the spec.
{
  const g = /a/g;
  g.lastIndex = 3;
  "aXa".replace(g, "b");
  console.log("replace g lastIndex", g.lastIndex);
  const y = /a/y;
  y.lastIndex = 1;
  console.log("replace y", "baa".replace(y, "_"), y.lastIndex);
  const m = /a/g;
  m.lastIndex = 2;
  console.log("match g", show("aa".match(m)), m.lastIndex);
  const sp = /,/g;
  sp.lastIndex = 1;
  console.log("split", show("a,b".split(sp)), sp.lastIndex);
}

// replaceAll with a non-global RegExp still throws.
try {
  "aa".replaceAll(/a/, "b");
  console.log("no throw");
} catch (e) {
  console.log("replaceAll non-global:", (e as Error).constructor.name);
}

// --- Each observable override is still observed.
const proto = RegExp.prototype as any;
const log: string[] = [];

// A patched flag getter is seen by `flags`, replace, match and split.
{
  const d = Object.getOwnPropertyDescriptor(proto, "global")!;
  Object.defineProperty(proto, "global", {
    get() {
      log.push("global");
      return d.get!.call(this);
    },
    configurable: true,
  });
  log.length = 0;
  const re: any = /\+/g;
  const f = re.flags;
  const r = s.replace(re, " ");
  const m = s.match(re);
  const p = s.split(re);
  console.log("patched global:", f, r, show(m), show(p), log.length);
  Object.defineProperty(proto, "global", d);
  log.length = 0;
  s.replace(/\+/g, " ");
  console.log("restored global:", s.replace(/\+/g, " "), log.length);
}

// A patched `flags` getter is seen by split, which reads it to build its
// splitter. (replace/match read `flags` too per spec, but V8 reads the flag
// getters there instead, so they are not compared against it here.)
{
  const d = Object.getOwnPropertyDescriptor(proto, "flags")!;
  Object.defineProperty(proto, "flags", {
    get() {
      log.push("flags");
      return "i";
    },
    configurable: true,
  });
  log.length = 0;
  console.log("patched flags:", show("aXbxc".split(/x/)), log.length);
  Object.defineProperty(proto, "flags", d);
  log.length = 0;
  console.log("restored flags:", show("aXbxc".split(/x/)), log.length);
}

// An own `flags` data property on the instance.
{
  const re: any = /x/;
  Object.defineProperty(re, "flags", { value: "i" });
  const plain: any = /x/;
  console.log("own flags:", re.flags, show("aXbxc".split(re)), show(plain.flags));
}

// Replaced prototype symbol methods are called.
for (const [name, sym] of [
  ["replace", Symbol.replace],
  ["match", Symbol.match],
  ["split", Symbol.split],
] as [string, symbol][]) {
  const saved = proto[sym];
  proto[sym] = function (this: RegExp, str: string) {
    return `${name}(${this.source},${str})`;
  };
  const re = /\+/g;
  const out =
    name === "replace" ? s.replace(re, "x") : name === "match" ? s.match(re) : s.split(re);
  proto[sym] = saved;
  console.log("proto", name, "->", show(out), "restored ->", show(name === "replace" ? s.replace(re, "x") : name === "match" ? s.match(re) : s.split(re)));
}

// An own symbol method on the instance.
{
  const re: any = /\+/g;
  re[Symbol.split] = () => "own split";
  re[Symbol.replace] = () => "own replace";
  re[Symbol.match] = () => "own match";
  console.log(s.split(re), s.replace(re, "x"), s.match(re));
}

// Subclasses: an own `exec`, and an own `@@split` on the subclass prototype.
let subclassExecs = 0;
class ExecR extends RegExp {
  exec(str: string) {
    subclassExecs++;
    return RegExp.prototype.exec.call(this, str);
  }
}
class SplitR extends RegExp {
  [Symbol.split](str: string) {
    return "SplitR " + str;
  }
}
{
  const re = new ExecR("\\+", "g");
  console.log("subclass exec:", s.replace(re, " "), show(s.match(re)), show(s.split(re)), subclassExecs > 0);
  const q = new SplitR("\\+", "g");
  console.log("subclass split:", s.split(q), s.replace(q, "_"));
}

// A replaced prototype `exec` is called by every operation.
{
  const saved = proto.exec;
  let calls = 0;
  proto.exec = function (this: RegExp, str: string) {
    calls++;
    return saved.call(this, str);
  };
  console.log("proto exec:", s.replace(/\+/g, " "), show(s.match(/\+/g)), show(s.split(/\+/)), calls > 0);
  proto.exec = saved;
}

// `RegExp.prototype.constructor` and `RegExp[Symbol.species]` feed split.
{
  const d = Object.getOwnPropertyDescriptor(RegExp, Symbol.species)!;
  Object.defineProperty(RegExp, Symbol.species, {
    get() {
      log.push("species");
      return d.get!.call(this);
    },
    configurable: true,
  });
  log.length = 0;
  console.log("patched species:", show(s.split(/\+/)), show(log));
  Object.defineProperty(RegExp, Symbol.species, d);

  const savedCtor = proto.constructor;
  const Fake: any = function () {};
  Fake[Symbol.species] = function (this: any, src: RegExp, flags: string) {
    log.push("construct " + flags);
    return new RegExp(src, flags);
  };
  proto.constructor = Fake;
  log.length = 0;
  console.log("patched constructor:", show(s.split(/\+/)), show(log));
  proto.constructor = savedCtor;
  log.length = 0;
  console.log("restored:", show(s.split(/\+/)), show(log));
}

// A `limit` whose valueOf changes the prototype mid-call.
{
  const saved = proto.exec;
  let calls = 0;
  const limit = {
    valueOf() {
      proto.exec = function (this: RegExp, str: string) {
        calls++;
        return saved.call(this, str);
      };
      return 10;
    },
  };
  console.log("limit valueOf:", show(s.split(/\+/, limit as any)), calls > 0);
  proto.exec = saved;
}

const all: any = /a/gimsuy;
console.log("final:", s.replace(/\+/g, " "), show(s.match(/\+/g)), show(s.split(/\+/)), all.flags);
