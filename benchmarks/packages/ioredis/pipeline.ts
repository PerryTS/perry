// ioredis: pipeline() batches of 20 commands (HSET/HGETALL/LPUSH/LRANGE).
import Redis from "ioredis";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(500, 50);
header("ioredis/pipeline", "ioredis", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  const r = new Redis({ host: HOST, port: port("PKG_BENCH_REDIS_PORT"), lazyConnect: true });
  r.on("error", (e: any) => console.log("client error " + (e && e.message)));
  await r.connect();
  const once = async (i: number, h: number): Promise<number> => {
    const key = "pkgbench:iop:" + (i % 50);
    const p = r.pipeline().del(key + ":l");
    for (let k = 0; k < 8; k++) p.hset(key + ":h", "f" + k, String(i * k)).lpush(key + ":l", "e" + k);
    p.hgetall(key + ":h").lrange(key + ":l", 0, 3);
    const res: any = await p.exec();
    const hash = res[res.length - 2][1];
    const list = res[res.length - 1][1];
    return fnv(h, res.length + ":" + hash.f7 + ":" + list.join(","));
  };
  let h = FNV_SEED;
  for (let i = 0; i < WARM; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < N; i++) h = await once(i, h);
  await r.quit();
  console.log("checksum " + hex(h));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
