// redis (node-redis): SET + GET + INCR round trips on one client.
import { createClient } from "redis";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(2000, 200);
header("redis/set_get", "redis", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  const c = createClient({ socket: { host: HOST, port: port("PKG_BENCH_REDIS_PORT") } });
  c.on("error", (e: any) => console.log("client error " + (e && e.message)));
  await c.connect();
  await c.del("pkgbench:nr:ctr");
  const once = async (i: number, h: number): Promise<number> => {
    await c.set("pkgbench:nr:" + (i % 500), "value-" + i);
    const v = await c.get("pkgbench:nr:" + (i % 500));
    const n = await c.incr("pkgbench:nr:ctr");
    return fnv(h, v + ":" + (n > 0));
  };
  let h = FNV_SEED;
  for (let i = 0; i < WARM; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < N; i++) h = await once(i, h);
  const total = await c.get("pkgbench:nr:ctr");
  await c.quit();
  console.log("checksum " + hex(h) + " ctr " + total);
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
