// lru-cache: mixed has/peek/delete/set with sizeCalculation + maxSize.
import { LRUCache } from "lru-cache";
import { iters, header, lcg } from "../_lib/bench.ts";

const it = iters(300000, 10000);
header("lru-cache/ttl_mixed", "lru-cache", it);
const cache = new LRUCache<number, string>({ maxSize: 20000, sizeCalculation: (v: string) => v.length });
const rnd = lcg(777);
let hasN = 0, peekLen = 0, dels = 0;
function op(i: number): void {
  const r = rnd();
  const k = r % 3000;
  switch ((r >>> 12) & 3) {
    case 0: if (cache.has(k)) hasN++; break;
    case 1: { const v = cache.peek(k); if (v !== undefined) peekLen += v.length; break; }
    case 2: if (cache.delete(k)) dels++; break;
    default: cache.set(k, "v".repeat(1 + (k % 23)) + i % 10);
  }
}
for (let i = 0; i < it.warm; i++) op(i);
hasN = 0; peekLen = 0; dels = 0;
for (let i = 0; i < it.n; i++) op(i);
console.log("has " + hasN + " peekLen " + peekLen + " dels " + dels + " size " + cache.size + " calc " + cache.calculatedSize);
