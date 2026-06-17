// `.index` and `.input` on a RegExp `exec()` / `String.match()` result must be
// real own properties on each result, not a most-recent-match thread-local. A
// stored result has to keep its `.index` after a later match on a different
// regex. Regression for the `.index` aliasing bug (companion to `.groups`).

function show(label: string, value: any) {
  console.log(label + " = " + String(value));
}

// exec(): stored index survives a later exec on another regex.
const a = /b/.exec("zzb"); // index 2
const aIndex = a?.index;
const b = /q/.exec("zq"); // index 1
show("exec stored aIndex", aIndex);
show("exec a.index after b", a?.index);
show("exec b.index", b?.index);
show("exec a.input", a?.input);

// match(): same, for the String.match path.
const m1 = "abc".match(/c/); // index 2
const m1Index = m1?.index;
const m2 = "xy".match(/y/); // index 1
show("match stored m1Index", m1Index);
show("match m1.index after m2", m1?.index);
show("match m2.index", m2?.index);
show("match m1.input", m1?.input);

// Inline reads.
show("inline exec index", /d/.exec("zzzd")?.index);
show("inline match index", "wxd".match(/d/)?.index);

// `.index` and `.groups` together, two regexes interleaved in a loop.
const reDigit = /(?<d>\d)/;
const reAlpha = /(?<c>[a-z])/;
let out = "";
for (const s of ["1x", "2y"]) {
  const rd = reDigit.exec(s);
  const ra = reAlpha.exec(s);
  out += `${rd?.index}${rd?.groups?.d}${ra?.groups?.c},`;
}
show("loop", out);

// No match -> null.
show("nomatch", String(/zz/.exec("ab")));
