// Searches the lazy automaton answers (bounds first, captures from the
// evaluator) must give exactly what the backtracking evaluator gives: empty
// matches, lastIndex advance over astral characters, sticky, split limits,
// global replace on short and long subjects, ASCII and non-ASCII storage.

function show(v: unknown): string {
  return JSON.stringify(v);
}

// Global replace on short fields (tar octal cleanup) and long subjects.
console.log(show("0000644\0 ".replace(/[\0 ]/g, "")));
console.log(show("12345670".replace(/[\0 ]/g, "")));
console.log(show("  a  b  ".replace(/\s+/g, "_")));
console.log(show(("ab ".repeat(40) + "é ".repeat(5)).replace(/b /g, "B")));
console.log(show("x%2Fy%2fz%zz".replace(/%[0-9a-f]{2}/gi, (m) => "<" + m + ">")));
console.log(show("a.b[c].d".replace(/\.([^.[]+)/g, "[$1]")));

// Empty matches advance by one unit, or one code point under u.
console.log(show("a😀b".replace(/(?:)/gu, "-")));
console.log(show("a😀b".replace(/(?:)/g, "-").length));
console.log(show("axxb".replace(/x*/g, "-")));
console.log(show("aaa".match(/a*?/g)));
console.log(show("x😀y".match(/./gu)));
console.log(show("x😀y".match(/./g)!.length));

// lastIndex: global exec loop over astral characters, sticky.
{
  const re = /\u{1F600}|b/gu;
  const s = "a😀b😀c";
  const seen: string[] = [];
  let m: RegExpExecArray | null;
  while ((m = re.exec(s)) !== null) seen.push(m.index + ":" + re.lastIndex);
  console.log(seen.join(","));
}
{
  const re = /a/y;
  re.lastIndex = 1;
  console.log(show([re.test("ba"), re.lastIndex, re.test("ba"), re.lastIndex]));
  const g = /o/g;
  console.log(show([g.test("foo"), g.lastIndex, g.test("foo"), g.lastIndex, g.test("foo"), g.lastIndex]));
}

// Split with limits and captures.
console.log(show("a, b ,c".split(/\s*,\s*/)));
console.log(show("abc".split(/(?:)/, 2)));
console.log(show("a1b22c".split(/(\d+)/)));
console.log(show("a1b22c".split(/\d+/, 1)));
console.log(show("-t, --tag <tags...>".split(/[ |,]+/)));

// Anchors, word boundaries, case folding, non-ASCII storage.
console.log(show("a\nb\nb".match(/^b/gm)));
console.log(show(/\bfo/.exec("afo fo")!.index));
console.log(show(/\Bo/.exec("o foo")!.index));
console.log(show("café éé".match(/é+/gu)));
console.log(show(/k/iu.exec("Kk")!.index));
console.log(show(/(foo|bar|baz)=(\d+)/.exec("a=1&baz=42&foo=7")));
console.log(show(/^-(\d+|\d*\.\d+)(e[+-]?\d+)?$/.test("-1.5e3")));
console.log(show(/^--[^=]+=/.test("--port=80")));
console.log(show("record_12345".search(/[0-9]+/)));
console.log(show("no match here".search(/q/)));
console.log(show([..."a1b2c3".matchAll(/([a-z])(\d)/g)].map((m) => m[1] + m[2] + "@" + m.index)));
