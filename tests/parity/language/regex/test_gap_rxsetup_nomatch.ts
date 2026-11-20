// RegExp replace and exec on subjects the pattern cannot match: the result,
// lastIndex after a failed builtin exec for every flag combination, the
// coercions that run even without a match, and subjects whose leading units
// appear only through case folding or past non-ASCII text.
const show = (label: string, value: unknown) => console.log(label, JSON.stringify(value));
const short = ["a", "ab", "user", "q", ""];
const long = "user[name]=alice&user[roles][]=admin&q=hello%20world%26more&tags[0]=a&tags[1]=b";

// The qs patterns, matching and not.
for (const s of [...short, long, "a+b", "%5b%5D", "x%5Bx", "&#38;", "?q=1", "a.b[c]"]) {
  show("plus " + s, s.replace(/\+/g, " "));
  show("open " + s, s.replace(/%5B/gi, "[").replace(/%5D/gi, "]"));
  show("hex " + s, s.replace(/%[0-9a-f]{2}/gi, (m: string) => "<" + m + ">"));
  show("entity " + s, s.replace(/&#(\d+);/g, (_m: string, n: string) => String.fromCharCode(+n)));
  show("prefix " + s, s.replace(/^\?/, ""));
  show("dots " + s, s.replace(/\.([^.[]+)/g, "[$1]"));
}

// lastIndex after no match, per flag set; the starting value is arbitrary.
for (const flags of ["", "g", "y", "gy", "i", "gi", "yi"]) {
  for (const start of [0, 3, 100, -1, 1.5]) {
    const r = new RegExp("\\+", flags);
    r.lastIndex = start;
    const out = "abcdef".replace(r, "_");
    show(`lastIndex replace /${flags}/ ${start}`, [out, r.lastIndex]);
    const t = new RegExp("\\+", flags);
    t.lastIndex = start;
    show(`lastIndex test /${flags}/ ${start}`, [t.test("abcdef"), t.lastIndex]);
    const e = new RegExp("\\+", flags);
    e.lastIndex = start;
    show(`lastIndex exec /${flags}/ ${start}`, [e.exec("abcdef"), e.lastIndex]);
  }
}
// A string or other non-Number lastIndex is still read (ToLength).
{
  const r: any = /\+/;
  let reads = 0;
  r.lastIndex = { valueOf() { reads++; return 2; } };
  show("lastIndex valueOf plain", ["abc".replace(r, "_"), reads]);
  const y: any = /\+/y;
  y.lastIndex = { valueOf() { reads++; return 1; } };
  show("lastIndex valueOf sticky", ["abc".replace(y, "_"), reads, y.lastIndex]);
  const g: any = /\+/g;
  g.lastIndex = { valueOf() { reads++; return 1; } };
  show("lastIndex valueOf global", ["abc".replace(g, "_"), reads, g.lastIndex]);
  const s: any = /\+/;
  s.lastIndex = "7";
  show("lastIndex string plain", ["abc".replace(s, "_"), s.lastIndex]);
}
// A non-writable lastIndex throws only where the failure writes it.
for (const flags of ["", "g", "y"]) {
  const r = new RegExp("\\+", flags);
  Object.defineProperty(r, "lastIndex", { writable: false, value: 0 });
  try {
    show(`readonly /${flags}/`, "abc".replace(r, "_"));
  } catch (e: any) {
    show(`readonly /${flags}/ throws`, e instanceof TypeError);
  }
  const f = Object.freeze(new RegExp("\\+", flags));
  try {
    show(`frozen /${flags}/`, f.test("abc"));
  } catch (e: any) {
    show(`frozen /${flags}/ throws`, e instanceof TypeError);
  }
}

// ToString of the subject and of a replacement object runs with no match.
{
  let calls: string[] = [];
  const subject = { toString() { calls.push("subject"); return "abc"; } };
  const replacement = { toString() { calls.push("replacement"); return "_"; } };
  show("coerce", [RegExp.prototype[Symbol.replace].call(/\+/g, subject, replacement), calls]);
  calls = [];
  show("coerce string", ["abc".replace(/\+/g, replacement as any), calls]);
  let called = 0;
  show("callback", ["abc".replace(/\+/g, () => { called++; return "x"; }), called]);
  show("number subject", RegExp.prototype[Symbol.replace].call(/\+/, 12345, "_"));
  show("undefined replacement", "a+b".replace(/\+/, undefined as any));
  show("symbol call", (/\+/g as any)[Symbol.replace]("abc", "_"));
  try {
    "abc".replace(/\+/g, Symbol("s") as any);
  } catch (e: any) {
    show("symbol replacement throws", e instanceof TypeError);
  }
}

// Leading units reached through case folding, alternation or optional terms.
show("kelvin iu", "K".replace(/k/iu, "K!"));
show("kelvin i", "K".replace(/k/i, "K!"));
show("long s iu", "ſ".replace(/s/giu, "S!"));
show("long s class iu", "ſ".replace(/[r-t]/giu, "S!"));
show("long s v", "ſ".replace(/s/iv, "S!"));
show("fold ascii", "ABC".replace(/b/gi, "_"));
show("alternation", "xa".replace(/b|a/, "_"));
show("optional", "x".replace(/\+?x/, "_"));
show("star", "x".replace(/a*x/, "_"));
show("count zero", "x".replace(/a{0}x/, "_"));
show("plus run", "bbbc".replace(/b+c/, "_"));
show("class range", "m".replace(/[a-z]/g, "_"));
show("digits", "a1".replace(/\d/g, "#"));
show("multiline", "a\n?b".replace(/^\?/m, "_"));
show("escaped bar", "x|".replace(/x\|/, "_"));
show("empty pattern", "abc".replace(new RegExp(""), "_"));

// Subjects past non-ASCII text, astral characters and lone surrogates.
show("non-ascii none", "héllo wörld".replace(/\+/g, " "));
show("non-ascii match", "hél+lo".replace(/\+/g, " "));
show("astral", "\u{1F600}+\u{1F600}".replace(/\+/g, " "));
show("lone surrogate", (String.fromCharCode(0xd800) + "+").replace(/\+/g, " "));
show("lone surrogate none", (String.fromCharCode(0xdc00) + "x").replace(/\+/g, " "));

// The same receiver across many short subjects keeps its lastIndex rules.
{
  const g = /\+/g;
  let out = "";
  for (let i = 0; i < 50; i++) out += ("s" + i + (i % 7 === 0 ? "+" : "")).replace(g, "_");
  show("loop", [out, g.lastIndex]);
  const y = /a/y;
  y.lastIndex = 1;
  show("sticky miss at lastIndex", ["ab".replace(y, "_"), y.lastIndex]);
  y.lastIndex = 0;
  show("sticky hit", ["ab".replace(y, "_"), y.lastIndex]);
}

// Shared exec entry: test, exec, match, search, split, matchAll, replaceAll.
show("match none", "abc".match(/\+/g));
show("match one none", "abc".match(/\+/));
show("search none", "abc".search(/\+/));
show("split none", "abc".split(/\+/));
show("matchAll none", [..."abc".matchAll(/\+/g)]);
show("replaceAll none", "abc".replaceAll(/\+/g, "_"));
try {
  "abc".replaceAll(/\+/, "_");
} catch (e: any) {
  show("replaceAll non-global throws", e instanceof TypeError);
}

// compile() replaces the pattern and its leading units.
{
  const r: any = /\+/g;
  show("before compile", "a-b".replace(r, "_"));
  r.compile("-", "g");
  show("after compile", ["a-b".replace(r, "_"), r.lastIndex]);
}
