// lru-cache: get/set churn over a key space 5x the cache capacity, with a
// deterministic LCG access pattern (hits, misses and evictions).
import { LRUCache } from "lru-cache";
import { iters, header, lcg } from "../_lib/bench.ts";

const it = iters(500000, 10000);
header("lru-cache/churn", "lru-cache", it);
const cache = new LRUCache<string, number>({ max: 1000 });
const keys: string[] = [];
for (let k = 0; k < 5000; k++) keys.push("key:" + k);
const rnd = lcg(12345);
let hits = 0, misses = 0, sum = 0;
function op(i: number): void {
  const k = keys[rnd() % 5000];
  const v = cache.get(k);
  if (v === undefined) { misses++; cache.set(k, i); } else { hits++; sum = (sum + v) % 1000000007; }
}
for (let i = 0; i < it.warm; i++) op(i);
hits = 0; misses = 0; sum = 0;
for (let i = 0; i < it.n; i++) op(i);
console.log("hits " + hits + " misses " + misses + " sum " + sum + " size " + cache.size);
