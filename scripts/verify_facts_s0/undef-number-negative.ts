// A local the facts prove Number-or-undefined gets its ToInt32 once
// (`expr::int32_shadow`). `mix`'s parameter also receives a string, so it is
// no such local. Under PERRY_FACTS_SABOTAGE=undefnum the facts claim every
// local that may hold a Number: the shadow's witness must report the string.
// A loop bound the compiler cannot fold keeps the loop as written.
const RUN = Math.random() < 2 ? 1 : 0;
function mix(v: any): number {
  return ((v & 255) ^ (v >> 3) ^ (v << 1)) | 0;
}
let s = 0;
for (let i = 0; i < 8 * RUN; i++) s += mix(i * 37);
s += mix("77");
console.log(s);
