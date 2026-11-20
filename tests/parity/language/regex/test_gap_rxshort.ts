function show(name: string, value: unknown) { console.log(name, JSON.stringify(value)); }
for (const source of ["abcdefghi", "abcd+efghi", "", "x".repeat(10000), "x".repeat(10000) + "+end"]) {
 for (const flags of ["g", "gy", "y", "gu", "guy", ""]) {
  const re = new RegExp("\\+", flags);
  for (const start of [0, 4, 999999, -1, -0]) {
   re.lastIndex = start;
   const output = source.replace(re, " ");
   show("replace", [flags, start, output.length, output.slice(-12), re.lastIndex, Object.is(re.lastIndex, -0)]);
   re.lastIndex = start;
   show("split", [flags, start, source.split(re, 3).map(s => s.length), re.lastIndex, Object.is(re.lastIndex, -0)]);
  }
 }
}
for (const source of ["abc", "a+b+c", "x".repeat(500) + "+tail"]) {
 for (const pattern of ["\\+", "(\\+)", "(?<plus>\\+)"]) {
  const re = new RegExp(pattern, "g");
  show("tokens", source.replace(re, "$$:$&:$1:$<plus>:$'").slice(-100));
 }
}
for (const source of ["a=1&b=2&c=3&d=4&e=5&f=6&g=7&h=8&i=9&j=0&k=1&l=2", "", "a&&b&", "😀&\ud800&x&\udc00", "x".repeat(10000) + "&tail"]) {
 for (const separator of ["&", "&&", "", "😀", "\ud800", "\udc00"]) {
  for (const limit of [undefined, 0, 1, 2, 12, 16, 17, 100, -1, NaN, Infinity, 4294967296]) {
   const parts = source.split(separator, limit);
   show("string split", [separator, String(limit), parts.length, parts.slice(0, 3).map(p => [p.length, p.charCodeAt(0), p.charCodeAt(p.length - 1)])]);
  }
 }
}
for (const value of [0, -0, 2]) {
 const re = /x/g;
 Object.defineProperty(re, "lastIndex", {writable: false, value});
 try { "abcdef".replace(re, "z"); show("readonly", "missed throw"); }
 catch (error) { show("readonly", error instanceof TypeError); }
}
const changed = /x/g;
const template = { toString() {
 Object.defineProperty(changed, "flags", {get() { show("flags getter", "called"); return ""; }});
 return "z";
}};
changed.lastIndex = 5;
show("coercion invalidates", ["xx".replace(changed, template as any), changed.lastIndex]);
const holder = /x/g;
const callback = (match: string) => { holder.lastIndex = 27; return "z"; };
show("callback final index", ["xx".replace(holder, callback), holder.lastIndex]);
show("unicode boundaries", ["😀".replace(/(?:)/gu, "_"), "😀".split(/(?:)/u), "😀".split(""), "😀".split("\udc00")]);
show("literal", ["a%5Bkey".replace("%5B", "["), "nomatch".replace("%5B", "["), "ababa".replaceAll("aba", "_")]);

for (const pattern of ["abcdefghijklmnop", "abcdefghijklmnopq", "aaaaaaaaaaaaaaaaab", ""]) {
 const source = "x" + pattern + pattern + "x";
 show("literal spill", [pattern.length, source.replace(pattern, "_"), source.replaceAll(pattern, "_")]);
}
show("many matches", "a+".repeat(40).replace(/\\+/g, "_"));
