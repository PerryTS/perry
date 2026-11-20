// The thread census again, on PostgreSQL rather than Redis, because "N
// connections cost N threads" should not rest on one protocol.
//
// 12 `pg` clients, each holding a 200 ms server-side sleep at the same moment.
// Under the pre-P7 transport each of those in-flight queries borrowed one tokio
// blocking-pool thread for its whole duration (`spawn_blocking` +
// `Handle::block_on`), so the in-flight row read 13; on turnloop they are
// 12 sockets on one loop and it reads 1.
//
// `pg_sleep` rather than a fast query on purpose: the measurement needs every
// query to still be outstanding when the census is taken, and a `SELECT 1`
// against a loopback server can finish before the next line runs.
//
// The clients are separate `const`s rather than an array: a method call whose
// receiver is an array element does not reach Perry's native-method table and
// silently returns `undefined`, on the base commit as well as on this branch.
//
//   PGHOST=127.0.0.1 PGPORT=55432 PGUSER=perry PGPASSWORD=perry_test PGDATABASE=perry_test
//
// parity-skip: requires a live PostgreSQL fixture
import { readdirSync, readFileSync } from "node:fs";
import { Client } from "pg";

const config = {
  host: process.env.PGHOST ?? "127.0.0.1",
  port: Number(process.env.PGPORT ?? "5432"),
  user: process.env.PGUSER ?? "postgres",
  password: process.env.PGPASSWORD ?? "",
  database: process.env.PGDATABASE ?? "postgres",
};

function names(): string {
  try {
    const counts = new Map<string, number>();
    for (const t of readdirSync("/proc/self/task")) {
      try {
        const n = readFileSync(`/proc/self/task/${t}/comm`, "utf8").trim();
        counts.set(n, (counts.get(n) ?? 0) + 1);
      } catch {}
    }
    const out: string[] = [];
    for (const [n, c] of counts) out.push(`${n} x${c}`);
    out.sort();
    return out.join(", ");
  } catch {
    return "unavailable";
  }
}

function count(): number {
  try {
    return readdirSync("/proc/self/task").length;
  } catch {
    return -1;
  }
}

const c0 = new Client(config);
const c1 = new Client(config);
const c2 = new Client(config);
const c3 = new Client(config);
const c4 = new Client(config);
const c5 = new Client(config);
const c6 = new Client(config);
const c7 = new Client(config);
const c8 = new Client(config);
const c9 = new Client(config);
const c10 = new Client(config);
const c11 = new Client(config);

async function main(): Promise<void> {
  console.log("connections:", 12);
  console.log("idle threads:", count());

  await Promise.all([
  c0.connect(),
  c1.connect(),
  c2.connect(),
  c3.connect(),
  c4.connect(),
  c5.connect(),
  c6.connect(),
  c7.connect(),
  c8.connect(),
  c9.connect(),
  c10.connect(),
  c11.connect()
  ]);
  console.log("connected threads:", count(), "|", names());

  // Every query submitted before any is awaited: this is the moment the old
  // transport needed 12 blocking threads at once.
  const inflight = [
  c0.query("SELECT pg_sleep(0.2), 0 AS n"),
  c1.query("SELECT pg_sleep(0.2), 1 AS n"),
  c2.query("SELECT pg_sleep(0.2), 2 AS n"),
  c3.query("SELECT pg_sleep(0.2), 3 AS n"),
  c4.query("SELECT pg_sleep(0.2), 4 AS n"),
  c5.query("SELECT pg_sleep(0.2), 5 AS n"),
  c6.query("SELECT pg_sleep(0.2), 6 AS n"),
  c7.query("SELECT pg_sleep(0.2), 7 AS n"),
  c8.query("SELECT pg_sleep(0.2), 8 AS n"),
  c9.query("SELECT pg_sleep(0.2), 9 AS n"),
  c10.query("SELECT pg_sleep(0.2), 10 AS n"),
  c11.query("SELECT pg_sleep(0.2), 11 AS n")
  ];
  console.log("in-flight threads:", count(), "|", names());
  const results = await Promise.all(inflight);
  let correct = 0;
  for (let i = 0; i < results.length; i++) {
    const rows = (results[i] as { rows: Array<{ n: number }> }).rows;
    if (rows.length === 1 && rows[0].n === i) correct++;
  }
  console.log("results:", correct, "of", 12);
  console.log("after threads:", count(), "|", names());

  await Promise.all([
  c0.end(),
  c1.end(),
  c2.end(),
  c3.end(),
  c4.end(),
  c5.end(),
  c6.end(),
  c7.end(),
  c8.end(),
  c9.end(),
  c10.end(),
  c11.end()
  ]);
  console.log("closed threads:", count());
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
