// `receiver[key]` with a numeric key on a Proxy whose static type is unknown
// runs the `get` trap with the canonical string key. Perry took the element
// path and read a word past the Proxy cell (0): React Flight's client-reference
// lookup `bundlerConfig[id]` with a numeric module id missed, and Next.js's
// dynamic pages threw "Could not find the module ... in the React Server
// Consumer Manifest".
const map: Record<string, any> = { "39756": { "*": { id: 2420 } } };
const config = new Proxy({}, { get(_t, k) { return map[k as string]; } });
const row = JSON.parse('[39756,["/x.js"],"default"]');

function resolve(e: any, t: any) {
  const r = e[t[0]];
  const hit = r && (r[t[2]] || r["*"]);
  return hit ? hit.id : "MISS " + JSON.stringify(r);
}
console.log(resolve(config, row));

const seen: string[] = [];
const spy = new Proxy({} as any, { get(_t, k) { seen.push(typeof k + ":" + String(k)); return 1; } });
function read(o: any, k: any) { return o[k]; }
read(spy, 0); read(spy, 7); read(spy, 4294967295); read(spy, -1); read(spy, 1.5); read(spy, "s");
console.log(seen.join(" "));

const arr = new Proxy([10, 20, 30], {});
console.log(read(arr, 1), read(arr, 5), read(arr, "length"));
const viaHandler = new Proxy([10, 20, 30], { get(t, k, r) { return k === "1" ? "one" : Reflect.get(t, k, r); } });
console.log(read(viaHandler, 1), read(viaHandler, 2));
