// A local the facts prove Number-or-undefined gets its ToInt32 once
// (`expr::int32_shadow`). `mix`'s parameter also receives a string, so it is
// no such local. Under PERRY_FACTS_SABOTAGE=undefnum the facts claim every
// local that may hold a Number: the shadow's witness must report the string.
// The values come from one Array, so every call site passes both kinds.
const xs: any[] = [37, 74, 111, "77", 5];
function mix(v: any): number {
  return ((v & 255) ^ (v >> 3) ^ (v << 1)) | 0;
}
let s = 0;
for (let i = 0; i < xs.length; i++) s += mix(xs[i]);
console.log(s);
