// #11519: more native entry points that took a string argument as a masked
// `*StringHeader` and so read a SHORT (SSO, <= 5 bytes, built at runtime)
// string's inline characters as an address: `JSON.parse(text, reviver)`,
// `new AggregateError(errors, message)`, `str.at(i)` / `str.codePointAt(i)`
// on a short receiver, and a well-known `Symbol[name]` lookup.
const S = (s: string): string => (s.length ? s.charAt(0) + s.slice(1) : JSON.parse('""'));

console.log("reviver:", JSON.stringify(JSON.parse(S("12"), (_k, v) => v)), JSON.parse(String(42), (_k, v) => v * 2));
console.log("reviver arr:", JSON.stringify(JSON.parse(S("[1,2]"), (_k, v) => (typeof v === "number" ? v + 1 : v))));
console.log("reviver empty-ish:", JSON.parse(S("0"), (_k, v) => v), JSON.parse(S("null"), (_k, v) => v));

for (const m of ["", "m", "boom", "12345", "123456"]) {
  const e = new AggregateError([1, 2], S(m));
  console.log(`aggregate len ${m.length}:`, JSON.stringify(e.message), e.errors.length, String(e));
}
console.log("aggregate tpl:", new AggregateError([], `${7}`).message);

for (const v of ["a", "ab", "abcde", "abcdef"]) {
  const s = S(v);
  console.log(`at/codePointAt len ${v.length}:`, s.at(0), s.at(-1), s.at(9), s.codePointAt(1), s.codePointAt(9));
}
const n = String(98765);
let acc = 0;
for (let i = 0; i < n.length; i++) acc += n.codePointAt(i)! + (n.at(i) === "5" ? 100 : 0);
console.log("loop:", acc);

console.log("Symbol[name]:", (Symbol as any)[S("match")] === Symbol.match, (Symbol as any)[S("split")] === Symbol.split);
