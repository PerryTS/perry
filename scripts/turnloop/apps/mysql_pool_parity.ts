// P7 acceptance: the `mysql2/promise` **pool** surface, which the connection
// fixture does not touch and which has no unit test — opening a pool member
// needs a real socket, so the pool's logic is reviewed rather than tested.
// This is the only end-to-end exercise it gets.
//
// What it checks, in order: a pooled one-shot query; a pinned connection from
// `getConnection()`; a transaction on that pinned connection, whose visibility
// is checked from the *pool* (a different member) while it is still open, so a
// pool that handed the same physical connection to both would report the
// uncommitted row; the release; and `pool.end()`.
//
//   MYSQL_HOST=127.0.0.1 MYSQL_PORT=53306 MYSQL_USER=perrynat
//   MYSQL_PASSWORD=perry_test MYSQL_DATABASE=perry_test
//
// parity-skip: requires a live MySQL fixture
import mysql from "mysql2/promise";

const config = {
  host: process.env.MYSQL_HOST ?? "127.0.0.1",
  port: Number(process.env.MYSQL_PORT ?? "3306"),
  user: process.env.MYSQL_USER ?? "root",
  password: process.env.MYSQL_PASSWORD ?? "",
  database: process.env.MYSQL_DATABASE ?? "test",
};

function show(label: string, rows: unknown): void {
  console.log(`${label}: ${JSON.stringify(rows)}`);
}

async function main(): Promise<void> {
  const pool = mysql.createPool(config);

  await pool.query("DROP TABLE IF EXISTS p7_mysql_pool");
  await pool.query("CREATE TABLE p7_mysql_pool (id INT, name VARCHAR(32))");
  await pool.query("INSERT INTO p7_mysql_pool VALUES (1, 'one')");
  await pool.query("INSERT INTO p7_mysql_pool VALUES (2, 'two')");

  const [seeded] = await pool.query("SELECT id, name FROM p7_mysql_pool ORDER BY id");
  show("pool-select", seeded);

  const [prepared] = await pool.execute("SELECT name FROM p7_mysql_pool WHERE id = ?", [2]);
  show("pool-execute", prepared);

  // Several pool queries in flight at once: on the old transport each borrowed
  // its own tokio blocking-pool thread, and on this one they share the pool's
  // members. Either way every answer must be the right one for its own query.
  const concurrent = await Promise.all([
    pool.query("SELECT 1 AS a"),
    pool.query("SELECT 2 AS a"),
    pool.query("SELECT 3 AS a"),
    pool.query("SELECT 4 AS a"),
  ]);
  const seen: number[] = [];
  for (const c of concurrent) {
    const rows = (c as unknown as Array<Array<{ a: number }>>)[0];
    seen.push(rows[0].a);
  }
  console.log("pool-concurrent:", seen.join(","));

  // A pinned connection, and a transaction on it.
  const conn = await pool.getConnection();
  await conn.beginTransaction();
  await conn.query("INSERT INTO p7_mysql_pool VALUES (3, 'uncommitted')");
  const [insideTx] = await conn.query("SELECT id FROM p7_mysql_pool WHERE id = 3");
  show("inside-tx", insideTx);

  // Read from the pool while the transaction is still open. A pool that handed
  // the same physical connection to both would see the uncommitted row.
  const [outsideTx] = await pool.query("SELECT id FROM p7_mysql_pool WHERE id = 3");
  show("outside-tx", outsideTx);

  await conn.rollback();
  conn.release();

  const [afterRollback] = await pool.query("SELECT id FROM p7_mysql_pool ORDER BY id");
  show("after-rollback", afterRollback);

  // The same again, committed, so the release really returned a usable member.
  const conn2 = await pool.getConnection();
  await conn2.beginTransaction();
  await conn2.query("INSERT INTO p7_mysql_pool VALUES (4, 'committed')");
  await conn2.commit();
  conn2.release();

  const [afterCommit] = await pool.query("SELECT id FROM p7_mysql_pool ORDER BY id");
  show("after-commit", afterCommit);

  await pool.query("DROP TABLE p7_mysql_pool");
  await pool.end();
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
