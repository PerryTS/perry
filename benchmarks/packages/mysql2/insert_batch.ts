// mysql2 (promise API): parameterized INSERTs via query(), then a batched
// aggregate + 100-row fetch every 50 inserts.
import mysql from "mysql2/promise";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(2000, 200);
header("mysql2/insert_batch", "mysql2", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  const c = await mysql.createConnection({ host: HOST, port: port("PKG_BENCH_MYSQL_PORT"), user: "bench", password: "bench", database: "bench" });
  await c.query("DROP TABLE IF EXISTS pkgbench_my_items");
  await c.query("CREATE TABLE pkgbench_my_items (id int AUTO_INCREMENT PRIMARY KEY, name varchar(64) NOT NULL, qty int NOT NULL, price decimal(10,2) NOT NULL)");
  let id = 0;
  const once = async (i: number, h: number): Promise<number> => {
    await c.query("INSERT INTO pkgbench_my_items (name, qty, price) VALUES (?, ?, ?)", ["item-" + i, i % 13, (i % 1000) / 4]);
    id++;
    if (id % 50 === 0) {
      const [agg]: any = await c.query("SELECT count(*) AS n, CAST(sum(qty) AS SIGNED) AS q, CAST(sum(price) AS CHAR) AS p FROM pkgbench_my_items");
      const [rows]: any = await c.query("SELECT id, name, qty FROM pkgbench_my_items ORDER BY id DESC LIMIT 100");
      h = fnv(h, agg[0].n + ":" + agg[0].q + ":" + agg[0].p + ":" + rows.length + ":" + rows[99 % rows.length].name);
    }
    return h;
  };
  let h = FNV_SEED;
  for (let i = 0; i < WARM; i++) h = await once(i, h);
  h = FNV_SEED;
  for (let i = 0; i < N; i++) h = await once(i, h);
  await c.query("DROP TABLE pkgbench_my_items");
  await c.end();
  console.log("checksum " + hex(h));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
