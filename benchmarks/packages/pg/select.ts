// pg: parameterized SELECT round trips on one connection.
import pg from "pg";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(2000, 200);
header("pg/select", "pg", it);

async function main(): Promise<void> {
  const c = new pg.Client({ host: HOST, port: port("PKG_BENCH_PG_PORT"), user: "bench", database: "bench" });
  await c.connect();
  const once = async (i: number, h: number): Promise<number> => {
    const r = await c.query("SELECT $1::int + 1 AS n, $2::text AS s, $1::int % 7 = 0 AS b", [i, "row-" + (i % 100)]);
    const row = r.rows[0];
    return fnv(h, row.n + ":" + row.s + ":" + row.b);
  };
  let h = FNV_SEED;
  for (let i = 0; i < it.warm; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < it.n; i++) h = await once(i, h);
  await c.end();
  console.log("checksum " + hex(h));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
