// mysql2 (promise API): prepared-statement SELECT round trips via execute().
import mysql from "mysql2/promise";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";
import { port, HOST } from "../_lib/env.ts";

const it = iters(2000, 200);
header("mysql2/select", "mysql2", it);

async function main(): Promise<void> {
  const c = await mysql.createConnection({ host: HOST, port: port("PKG_BENCH_MYSQL_PORT"), user: "bench", password: "bench", database: "bench" });
  const once = async (i: number, h: number): Promise<number> => {
    const [rows]: any = await c.execute("SELECT ? + 1 AS n, ? AS s, (? % 7) = 0 AS b", [i, "row-" + (i % 100), i]);
    const row = rows[0];
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
