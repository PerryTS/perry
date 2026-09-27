// redis (node-redis): MULTI/EXEC batches of 20 commands (HSET/HGETALL/LPUSH/LRANGE).
import { createClient } from "redis";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(500, 50);
header("redis/pipeline", "redis", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  const c = createClient({ socket: { host: HOST, port: port("PKG_BENCH_REDIS_PORT") } });
  c.on("error", (e: any) => console.log("client error " + (e && e.message)));
  await c.connect();
  const once = async (i: number, h: number): Promise<number> => {
    const key = "pkgbench:nrp:" + (i % 50);
    let m = c.multi().del(key + ":l");
    for (let k = 0; k < 8; k++) m = m.hSet(key + ":h", "f" + k, String(i * k)).lPush(key + ":l", "e" + k);
    m = m.hGetAll(key + ":h").lRange(key + ":l", 0, 3);
    const res: any[] = await m.exec();
    const hash = res[res.length - 2];
    const list = res[res.length - 1];
    return fnv(h, res.length + ":" + hash.f7 + ":" + list.join(","));
  };
  let h = FNV_SEED;
  for (let i = 0; i < WARM; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < N; i++) h = await once(i, h);
  await c.quit();
  console.log("checksum " + hex(h));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
