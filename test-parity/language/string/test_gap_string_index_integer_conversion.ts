export {};
const s: any = "a😀z";
const indices: any[] = [NaN, undefined, null, true, false, "2.9", -0.9, 0.9, 1.9, -1.9, Infinity, -Infinity, 2147483648, -2147483649];
for (const index of indices) {
  // Compare code units directly: console formatting of lone surrogates is
  // separate from index coercion and differs between the two runtimes.
  const character = s.charAt(index);
  const sliced = s.slice(index, 3);
  console.log(s.charCodeAt(index), character.length, character.charCodeAt(0),
    sliced.length, sliced.charCodeAt(0), sliced.charCodeAt(sliced.length - 1));
}
let count = 0;
const index: any = { valueOf() { count++; return 2.9; } };
console.log("coerced", s.charCodeAt(index), count);
try { s.charCodeAt(Symbol("index")); console.log("symbol missed"); }
catch (e: any) { console.log("symbol", e.name); }
console.log("end", s.slice(1, undefined), s.substring(1, undefined));
