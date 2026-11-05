// String.prototype.replace/replaceAll build every result in one output:
// string and RegExp patterns, templates and callbacks, empty replacements,
// global, sticky and plain receivers. Each row prints JSON so lone
// surrogates and empty strings are visible.
const show = (label: string, value: unknown) => console.log(label, JSON.stringify(value));

// Empty replacement, the former special case, on every receiver kind.
show("strip g", "0000644\0 ".replace(/[\0 ]/g, ""));
show("strip once", "a b c".replace(/ /, ""));
show("strip none", "abc".replace(/x/g, ""));
show("strip all", "   ".replace(/ /g, ""));
show("strip empty matches", "abc".replace(/(?:)/g, ""));
show("strip tail", "name\0\0\0".replace(/\0+$/, ""));
show("strip dotall", "base\0junk\nmore".replace(/\0.*$/s, ""));

// Templates.
show("tmpl $1", "foo-bar_baz".replace(/(\w+)[-_](\w+)/g, "$2.$1"));
show("tmpl $&", "ab12 cd345;".replace(/[0-9]+/g, "[$&]"));
show("tmpl $` $'", "abc".replace(/b/, "[$`|$']"));
show("tmpl $$", "a.b".replace(/\./g, "$$"));
show("tmpl $0 $9 $10", "abcdefghijk".replace(/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)/, "$0|$9|$10|$11|$01"));
show("tmpl unset capture", "b".replace(/(a)?b/, "[$1]"));
show("tmpl $<n> without names", "ab".replace(/(a)/, "$<n>"));
show("tmpl named", "John Smith".replace(/(?<first>\w+) (?<last>\w+)/, "$<last>, $<first>"));
show("tmpl named missing", "ab".replace(/(?<x>a)/, "[$<y>]"));
show("tmpl trailing $", "ab".replace(/a/, "x$"));
show("tmpl non-ascii", "aXbXc".replace(/X/g, "é$&ü"));
show("tmpl astral", "a-b".replace(/-/, "😀$&😀"));

// Callbacks, including the arguments and ToString of every result.
show("fn caps", "foo-bar_baz".replace(/[-_](\w)/g, (_m: string, c: string) => c.toUpperCase()));
show("fn args", "xaybz".replace(/(a)|(b)/g, (...args: unknown[]) => "<" + args.slice(0, -1).map(String).join(",") + ">"));
show("fn number", "a1b2".replace(/\d/g, (d: string) => (Number(d) * 2) as unknown as string));
let calls = 0;
const obj = { toString() { calls++; return "T"; } };
show("fn toString", "aaa".replace(/a/g, () => obj as unknown as string));
show("fn toString calls", calls);

// Surrogates split across pieces join into one code point.
const hi = "\uD83D", lo = "\uDE00";
const joined = (hi + "x" + lo).replace("x", "");
show("seam string", [joined, joined.length, joined.codePointAt(0)]);
const joinedRe = (hi + "x" + lo).replace(/x/g, "");
show("seam regexp", [joinedRe, joinedRe.length, joinedRe.codePointAt(0)]);
const joinedTmpl = "a".replace(/a/, hi + "$&" + lo).replace("a", "");
show("seam template", [joinedTmpl.length, joinedTmpl.codePointAt(0)]);
const joinedFn = ("x" + lo).replace("x", () => hi);
show("seam callback", [joinedFn.length, joinedFn.codePointAt(0)]);
show("lone kept", [(hi + "x").replace("x", "y"), ("x" + lo).replace("x", "y")]);
show("non-ascii subject", "çaçbçc".replace(/ç/g, "|"));
show("astral subject", "😀a😀b".replace(/a|b/g, "-"));

// String patterns.
show("str replaceAll", "a\\b\\c.txt".replaceAll("\\", "/"));
show("str replace first", "one-two-three".replace("-", "+"));
show("str overlapping", "aaaa a aaa".replaceAll("aa", "b"));
show("str kmp", "abababcabab".replaceAll("abab", "X"));
show("str empty pattern", "abc".replaceAll("", "-"));
show("str empty first", "abc".replace("", "-"));
show("str template", "a.b".replaceAll(".", "[$&$`$']"));
show("str callback", "a.b.c".replaceAll(".", (m: string, p: number, s: string) => `${m}${p}${s.length}`));
show("str none", "abc".replace("x", "y"));
show("str non-ascii", "ça.ça".replaceAll("ça", "x"));
show("short result", "abcdef".replace("bcdef", ""));

// lastIndex: reset for global, written once by a sticky receiver.
const g = /a/g; g.lastIndex = 3;
show("global result", "aXa".replace(g, "b"));
show("global lastIndex", g.lastIndex);
const y = /b/y; y.lastIndex = 1;
show("sticky hit", "abc".replace(y, "_"));
show("sticky lastIndex", y.lastIndex);
y.lastIndex = 0;
show("sticky miss", "abc".replace(y, "_"));
show("sticky miss lastIndex", y.lastIndex);
y.lastIndex = 9;
show("sticky past end", "abc".replace(y, "_"));
show("sticky past end lastIndex", y.lastIndex);
const gy = /a/gy;
show("global sticky", "aab".replace(gy, "x"));
show("global sticky lastIndex", gy.lastIndex);
const plain = /b/; plain.lastIndex = 7;
show("plain keeps lastIndex", ["abc".replace(plain, "_"), plain.lastIndex]);

// A non-Number lastIndex is read (ToLength) by a non-global exec.
let reads = 0;
const valued = /b/;
(valued as unknown as { lastIndex: unknown }).lastIndex = { valueOf() { reads++; return 0; } };
show("lastIndex valueOf", ["abc".replace(valued, "_"), reads]);

// A non-writable lastIndex throws on the global reset and on a sticky write.
const frozen = /a/g;
Object.defineProperty(frozen, "lastIndex", { writable: false, value: 0 });
try { "a".replace(frozen, "b"); show("frozen global", "no throw"); } catch (e) { show("frozen global", (e as Error).constructor.name); }
const frozenY = /a/y;
Object.defineProperty(frozenY, "lastIndex", { writable: false, value: 0 });
try { "a".replace(frozenY, "b"); show("frozen sticky", "no throw"); } catch (e) { show("frozen sticky", (e as Error).constructor.name); }

// A patched exec is observable: it takes the generic path, in spec order.
const log: string[] = [];
const patched = /a/g;
const builtinExec = RegExp.prototype.exec;
(patched as unknown as { exec: unknown }).exec = function (this: RegExp, s: string) {
  log.push("exec@" + this.lastIndex);
  return builtinExec.call(this, s);
};
show("patched result", "aXa".replace(patched, "b"));
show("patched log", log);
const custom = /x/g;
(custom as unknown as { exec: unknown }).exec = (() => {
  let n = 0;
  return () => (n++ === 0 ? Object.assign(["zz", "q"], { index: 1 }) : null);
})();
show("custom result", "abcd".replace(custom, "<$&|$1|$'>"));

// A replacer that changes the receiver cannot change which matches are replaced.
const meddled = /o/g;
show("meddle", "foo boo".replace(meddled, (m: string) => {
  meddled.lastIndex = 0;
  (meddled as unknown as { exec: unknown }).exec = () => null;
  return m.toUpperCase();
}));

// replaceAll requires a global RegExp.
try { "a".replaceAll(/a/, "b"); show("replaceAll non-global", "no throw"); } catch (e) { show("replaceAll non-global", (e as Error).constructor.name); }
show("replaceAll global", "a-a".replaceAll(/a/g, "$&$&"));

// Longer subjects cross the poll stride and the buffer's first size.
const long = "ab".repeat(5000);
const longOut = long.replace(/b/g, "cd");
show("long", [longOut.length, longOut.slice(0, 8), longOut.slice(-8)]);
const longStr = long.replaceAll("a", "");
show("long str", [longStr.length, longStr.slice(0, 4)]);
const longFn = long.replace(/a/g, () => "é");
show("long fn", [longFn.length, longFn.slice(0, 4)]);
