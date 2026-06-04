// Named-capture `.groups` must be a real own property on each exec/match
// result, not a single most-recent-match thread-local. A stored groups object
// (or a `.groups` read on a stored result) has to survive a later match on a
// different regex, and two results with different named captures must not
// share a shape. Regression for the `.groups` aliasing bug.

function show(label: string, value: any) {
  console.log(label + " = " + String(value));
}

// Inline read off the call result.
show("inline", /(?<y>\d{4})/.exec("2024")?.groups?.y);

// Stored result, immediate read (also checks .index / .input regressions).
const m = /(?<year>\d{4})-(?<month>\d{2})/.exec("2024-05");
show("stored.year", m?.groups?.year);
show("stored.month", m?.groups?.month);
show("stored.index", m?.index);
show("stored.input", m?.input);
show("stored[1]", m?.[1]);

// Aliased groups object survives a later exec on another regex.
const a = /(?<x>\d)/.exec("7");
const aGroups = a?.groups;
const b = /(?<z>\d)/.exec("9");
show("aliased.x", (aGroups as any)?.x);
show("later.z", b?.groups?.z);
show("later.index", b?.index);

// string.match() carries groups too.
show("match.year", "2024-05".match(/(?<year>\d{4})/)?.groups?.year);

// No named groups -> groups is undefined (not an empty object).
const plain = /(\d+)/.exec("42");
show("plain.groups", String(plain?.groups));
show("plain[1]", plain?.[1]);

// matchAll yields independent per-match groups.
const all = [..."2024 2025".matchAll(/(?<y>\d{4})/g)];
show("all[0].y", all[0]?.groups?.y);
show("all[1].y", all[1]?.groups?.y);

// Two regexes interleaved in a loop.
const reDigit = /(?<d>\d)/;
const reAlpha = /(?<c>[a-z])/;
let out = "";
for (const s of ["1x", "2y"]) {
  const rd = reDigit.exec(s);
  const ra = reAlpha.exec(s);
  out += (rd?.groups?.d ?? "?") + (ra?.groups?.c ?? "?") + ",";
}
show("loop", out);

// No match -> null.
show("nomatch", String(/(?<q>z)/.exec("abc")));
