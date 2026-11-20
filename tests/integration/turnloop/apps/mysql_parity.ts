// P7 acceptance: the `mysql2/promise` surface, run identically on Perry and on
// Node 26.5.1 with the real npm `mysql2`, against the same MySQL server.
//
// The user this connects as authenticates with `mysql_native_password`. A
// `caching_sha2_password` user exercises a different branch of the handshake —
// including the RSA seed the host has to supply from real entropy — and is a
// separate run (`MYSQL_USER=perry`), not a separate file.
//
// Deliberately avoided, because Perry and npm mysql2 disagree about them
// *independently of this migration*: DECIMAL (Perry returns a number, mysql2 a
// string), BIGINT beyond 2^53, BLOB (Perry returns a lossy string, mysql2 a
// Buffer), and DATE/DATETIME (Perry returns a formatted string, mysql2 a Date).
// Printing those would make this file assert those gaps rather than the
// transport.
//
//   MYSQL_HOST=127.0.0.1 MYSQL_PORT=53306 MYSQL_USER=perrynat
//   MYSQL_PASSWORD=perry_test MYSQL_DATABASE=perry_test
//
// parity-skip: requires a live MySQL fixture
import mysql from "mysql2/promise";

// Perry's `parse_mysql_config` reads the config object's fields
// **positionally** (host, port, user, password, database), so the key order
// here is load-bearing on Perry and irrelevant on Node.
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
  const conn = await mysql.createConnection(config);

  await conn.query("DROP TABLE IF EXISTS p7_mysql");
  await conn.query(
    "CREATE TABLE p7_mysql (id INT, name VARCHAR(64), flag TINYINT(1), ratio DOUBLE)",
  );

  const [ins] = await conn.query("INSERT INTO p7_mysql VALUES (1, 'alpha', 1, 1.5)");
  console.log("insert-affected:", (ins as { affectedRows: number }).affectedRows);
  await conn.query("INSERT INTO p7_mysql VALUES (2, 'bêta', 0, -0.25)");
  await conn.query("INSERT INTO p7_mysql VALUES (3, NULL, NULL, NULL)");

  const [all] = await conn.query("SELECT id, name, flag, ratio FROM p7_mysql ORDER BY id");
  show("select-all", all);

  const [empty] = await conn.query("SELECT id FROM p7_mysql WHERE id = 999");
  show("select-empty", empty);

  // `execute` forces a prepared statement: a different wire sequence (prepare
  // then execute, binary result rows) and the place a queue bug shows up as a
  // hang rather than a wrong answer.
  const [one] = await conn.execute("SELECT name FROM p7_mysql WHERE id = ?", [2]);
  show("execute-param", one);
  const [byName] = await conn.execute("SELECT id FROM p7_mysql WHERE name = ?", ["alpha"]);
  show("execute-name", byName);

  // A value wider than one read, so the core has to reassemble a row that
  // arrives in pieces.
  const [wide] = await conn.query("SELECT REPEAT('z', 70000) AS wide");
  const wideRows = wide as Array<{ wide: string }>;
  console.log("wide-len:", wideRows.length === 1 ? wideRows[0].wide.length : -1);

  const [upd] = await conn.query("UPDATE p7_mysql SET flag = 1 WHERE id = 2");
  console.log("update-affected:", (upd as { affectedRows: number }).affectedRows);

  // A statement error must reject and leave the connection usable.
  let failed = "no";
  try {
    await conn.query("SELECT * FROM p7_no_such_table");
  } catch (e) {
    failed = e instanceof Error && e.message.length > 0 ? "yes" : "empty";
  }
  console.log("error-rejected:", failed);
  const [after] = await conn.query("SELECT id FROM p7_mysql ORDER BY id");
  show("after-error", after);

  // A transaction pins the connection.
  await conn.beginTransaction();
  await conn.query("INSERT INTO p7_mysql VALUES (4, 'in-tx', 1, 4.0)");
  const [inTx] = await conn.query("SELECT id FROM p7_mysql WHERE id = 4");
  show("in-tx", inTx);
  await conn.rollback();
  const [afterRollback] = await conn.query("SELECT id FROM p7_mysql WHERE id = 4");
  show("after-rollback", afterRollback);

  await conn.beginTransaction();
  await conn.query("INSERT INTO p7_mysql VALUES (5, 'committed', 1, 5.0)");
  await conn.commit();
  const [afterCommit] = await conn.query("SELECT id FROM p7_mysql WHERE id = 5");
  show("after-commit", afterCommit);

  await conn.query("DROP TABLE p7_mysql");
  await conn.end();
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
