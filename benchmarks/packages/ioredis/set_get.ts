// ioredis: SET + GET + INCR round trips on one client.
import Redis from "ioredis";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(2000, 200);
header("ioredis/set_get", "ioredis", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  const r = new Redis({ host: HOST, port: port("PKG_BENCH_REDIS_PORT"), lazyConnect: true });
  r.on("error", (e: any) => console.log("client error " + (e && e.message)));
  await r.connect();
  await r.del("pkgbench:io:ctr");
  const once = async (i: number, h: number): Promise<number> => {
    await r.set("pkgbench:io:" + (i % 500), "value-" + i);
    const v = await r.get("pkgbench:io:" + (i % 500));
    const n = await r.incr("pkgbench:io:ctr");
    return fnv(h, v + ":" + (n > 0));
  };
  let h = FNV_SEED;
  for (let i = 0; i < WARM; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < N; i++) h = await once(i, h);
  const total = await r.get("pkgbench:io:ctr");
  await r.quit();
  console.log("checksum " + hex(h) + " ctr " + total);
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
