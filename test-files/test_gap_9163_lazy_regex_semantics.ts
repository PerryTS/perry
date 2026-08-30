// Compiling a RegExp's program is deferred to its first match (see
// `crates/perry-runtime/src/regex/lazy.rs`). Everything a program can observe
// about a RegExp WITHOUT matching must be unaffected by that, so this fixture
// exercises the observable surface that the deferral could plausibly break —
// and is diffed byte-for-byte against node.
//
// The five things at risk:
//   1. a syntactically invalid pattern must still throw SyntaxError at the
//      point of construction, not silently at first use (and not at all);
//   2. `.source` / `.flags` / the individual flag getters must be readable
//      before any match has happened, and must not change once one has;
//   3. `/g` and `/y` statefulness — `lastIndex` reads, writes and resets —
//      must survive the program being installed mid-life;
//   4. identity: two evaluations of the same literal are distinct objects
//      that share nothing (expandos, lastIndex);
//   5. the fancy-regex (lookbehind/backreference) and ECMAScript
//      RepeatMatcher fallbacks are installed by the deferred build too, not
//      only the linear engine's program.

// ---- 1. SyntaxError still comes from construction ---------------------------
// The literal forms are compile-time-known, so use the constructor to keep the
// throw at a runtime point both engines agree on.
const bad = ["(", "[z-a]", "a{2,1}", ")", "[", "\\p{Bogus}"];
for (const src of bad) {
  try {
    const flags = src === "\\p{Bogus}" ? "u" : "";
    new RegExp(src, flags);
    console.log("no-throw:" + src);
  } catch (e) {
    console.log("threw:" + src + ":" + (e instanceof SyntaxError));
  }
}

// An invalid pattern that is constructed but never matched with must throw all
// the same — the throw is not allowed to move to the (never reached) first use.
let neverMatched = 0;
try {
  const re = new RegExp("(unclosed");
  neverMatched = re.source.length; // unreachable
} catch (e) {
  neverMatched = e instanceof SyntaxError ? 1 : 2;
}
console.log("never-matched-still-throws:" + neverMatched);

// ---- 2. source / flags / getters without ever matching ----------------------
const meta = /[A-Za-zÀ-ɏ]+(?:foo|bar)[0-9]{1,4}/giu;
console.log("source:" + meta.source);
console.log("flags:" + meta.flags);
console.log(
  "getters:" +
    [meta.global, meta.ignoreCase, meta.multiline, meta.sticky, meta.unicode, meta.dotAll].join(","),
);
console.log("lastIndex-initial:" + meta.lastIndex);
console.log("toString:" + meta.toString());
// Empty pattern still reports the spec's `(?:)` source without matching.
console.log("empty-source:" + new RegExp("").source);
// ...and none of the above may change after the program is finally built.
console.log("after-exec-source:" + (meta.exec("xfoo12"), meta.source));
console.log("after-exec-flags:" + meta.flags);

// ---- 3. /g and /y statefulness across the deferred build --------------------
const g = /a/g;
console.log("g-before:" + g.lastIndex);
g.lastIndex = 2; // written BEFORE the program exists
console.log("g-preset-exec:" + JSON.stringify(g.exec("aaaa")));
console.log("g-after:" + g.lastIndex);
g.lastIndex = 0;
const seen: number[] = [];
let m: RegExpExecArray | null;
while ((m = g.exec("a-a-a")) !== null) seen.push(m.index);
console.log("g-walk:" + seen.join(",") + " reset:" + g.lastIndex);

const sticky = /b/y;
sticky.lastIndex = 1;
console.log("y-hit:" + sticky.test("ab") + " idx:" + sticky.lastIndex);
sticky.lastIndex = 0;
console.log("y-miss:" + sticky.test("ab") + " idx:" + sticky.lastIndex);

// A non-global regex must NOT touch lastIndex, built or not.
const plain = /c/;
plain.lastIndex = 7;
console.log("plain-test:" + plain.test("abc") + " idx:" + plain.lastIndex);

// ---- 4. identity: a fresh object per evaluation -----------------------------
function mk() {
  return /dup/g;
}
const p = mk();
const q = mk();
console.log("distinct:" + (p !== q));
p.lastIndex = 3;
console.log("independent-lastIndex:" + (q.lastIndex === 0));
(p as unknown as { tag?: string }).tag = "first";
console.log("independent-expando:" + ((q as unknown as { tag?: string }).tag === undefined));
// Two never-matched siblings are still distinct.
console.log("distinct-unmatched:" + (/never/ !== /never/));

// ---- 5. the fallback engines are installed by the deferred build too --------
const lookbehind = /(?<=pre)\d+/;
console.log("lookbehind:" + JSON.stringify("pre77".match(lookbehind)));
console.log("lookbehind-miss:" + lookbehind.test("post77"));
const backref = /(\w)\1/;
console.log("backref:" + JSON.stringify("abba".match(backref)));
const quantifiedCapture = /(?:(a)|(b))+/;
console.log("repeat-matcher:" + JSON.stringify("ab".match(quantifiedCapture)));

// String methods that take a RegExp reach the same deferred build.
console.log("replace:" + "a1b22c".replace(/\d+/g, "#"));
console.log("split:" + JSON.stringify("a1b22c".split(/\d+/)));
console.log("search:" + "xxabc".search(/abc/));
console.log("matchAll:" + JSON.stringify([..."a1b2".matchAll(/(\w)(\d)/g)].map((x) => x[0])));
console.log("named:" + JSON.stringify("2026-08".match(/(?<y>\d{4})-(?<mo>\d{2})/)?.groups));

// RegExp.prototype.compile re-initialises a header whose program was never
// built (the old pointer it releases is null in that case).
const recompiled = /zzz/g;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
(recompiled as any).compile("q+", "g");
console.log("compiled-source:" + recompiled.source + " flags:" + recompiled.flags);
console.log("compiled-match:" + JSON.stringify("aqqqb".match(recompiled)));
