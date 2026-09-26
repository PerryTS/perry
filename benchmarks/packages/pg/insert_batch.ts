// pg: parameterized INSERTs into a fresh table, then a batched multi-row
// SELECT (aggregate + 100-row fetch) every 50 inserts.
import pg from "pg";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(2000, 200);
header("pg/insert_batch", "pg", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  const c = new pg.Client({ host: HOST, port: port("PKG_BENCH_PG_PORT"), user: "bench", database: "bench" });
  await c.connect();
  await c.query("DROP TABLE IF EXISTS pkgbench_pg_items");
  await c.query("CREATE TABLE pkgbench_pg_items (id serial PRIMARY KEY, name text NOT NULL, qty int NOT NULL, price numeric(10,2) NOT NULL)");
  let id = 0;
  const once = async (i: number, h: number): Promise<number> => {
    await c.query("INSERT INTO pkgbench_pg_items (name, qty, price) VALUES ($1, $2, $3)", ["item-" + i, i % 13, (i % 1000) / 4]);
    id++;
    if (id % 50 === 0) {
      const agg = await c.query("SELECT count(*)::int AS n, sum(qty)::int AS q, sum(price)::text AS p FROM pkgbench_pg_items");
      const rows = await c.query("SELECT id, name, qty FROM pkgbench_pg_items ORDER BY id DESC LIMIT 100");
      h = fnv(h, agg.rows[0].n + ":" + agg.rows[0].q + ":" + agg.rows[0].p + ":" + rows.rows.length + ":" + rows.rows[99 % rows.rows.length].name);
    }
    return h;
  };
  let h = FNV_SEED;
  for (let i = 0; i < WARM; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < N; i++) h = await once(i, h);
  await c.query("DROP TABLE pkgbench_pg_items");
  await c.end();
  console.log("checksum " + hex(h));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
