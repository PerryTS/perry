// `names` maps Number keys to String values, so `names.get(k)` is a String
// or `undefined`. Under PERRY_FACTS_SABOTAGE=mapvals the facts take `set`'s
// key for its value (`Model::map_set`): `get` is claimed to return the
// Number keys, `use`'s parameter becomes an int32, and the witness must
// report the String that arrives.
// A bound the compiler cannot fold keeps each call site one HIR call.
const RUN = Math.random() < 2 ? 1 : 0;
const names = new Map<number, string>();
function use(m: number): number {
  return (m & 127) + 1;
}
function step(k: number): number {
  const v: any = names.get(k);
  names.set(k, "v" + k);
  return use(v === undefined ? 0 : v);
}
let s = 0;
for (let i = 0; i < 12 * RUN; i++) s += step(i & 3);
console.log(s);
