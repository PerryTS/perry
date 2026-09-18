// P7 acceptance: the `pg` surface, run identically on Perry and on Node 26.5.1
// with the real npm `pg`, against the same PostgreSQL server.
//
// The server this was written against authenticates with **scram-sha-256**, so
// a passing run is also the only evidence that the host-side SCRAM handshake
// works: the sans-I/O core asks for a `ScramSha256` because its constructor
// reads entropy, and Perry builds it in the binding.
//
// Deliberately avoided, because Perry and node-postgres disagree about them
// *independently of this migration* — each was reproduced on the base commit:
//
//   * `client.query(sql, params)` never reaches the server with its parameters.
//     Both transports answer "bind message supplies 0 parameters, but prepared
//     statement requires 1"; the base arm names sqlx's statement and this one
//     names the unnamed one, which is the only difference.
//   * `rowCount` on a non-SELECT. node-postgres reports the affected rows and
//     `null` for DDL; Perry reports 0. (On the base commit the whole result
//     object reads back `undefined` from TypeScript, so this is a improvement
//     rather than a regression — see the P7 report.)
//   * `int8` (Perry a number, pg a decimal string), `numeric` (Perry now a
//     number where sqlx produced null, pg a string), and the date/time and json
//     families (Perry null on both transports).
//
// Printing any of those would make this file assert those gaps rather than the
// transport.
//
//   PGHOST=127.0.0.1 PGPORT=55432 PGUSER=perry PGPASSWORD=perry_test PGDATABASE=perry_test
//
// parity-skip: requires a live PostgreSQL fixture
// Named imports rather than `const { Client } = pg`: destructuring a native
// module's default export is not something Perry's `pg` binding surface
// supports, and npm `pg` exports both spellings.
import { Client, Pool } from "pg";

// Perry's `parse_pg_config` reads the config object's fields **positionally**
// (host, port, user, password, database), so the key order here is load-bearing
// on Perry and irrelevant on Node.
const config = {
  host: process.env.PGHOST ?? "127.0.0.1",
  port: Number(process.env.PGPORT ?? "5432"),
  user: process.env.PGUSER ?? "postgres",
  password: process.env.PGPASSWORD ?? "",
  database: process.env.PGDATABASE ?? "postgres",
};

type Res = { rows: unknown[]; rowCount: number | null; command: string };

// `command` and `rows` are what the transport decides. `rowCount` is printed
// only for a SELECT, where Perry and node-postgres agree that it is the number
// of rows returned; on a non-SELECT it is one of the pre-existing divergences
// listed above.
function show(label: string, res: Res) {
  console.log(`${label}: command=${res.command} rows=${JSON.stringify(res.rows)}`);
}

function showSelect(label: string, res: Res) {
  console.log(`${label}: command=${res.command} rowCount=${res.rowCount} rows=${JSON.stringify(res.rows)}`);
}

async function main(): Promise<void> {
  const client = new Client(config);
  await client.connect();

  await client.query("DROP TABLE IF EXISTS p7_pg");
  show("create", await client.query("CREATE TABLE p7_pg (id int4, name text, flag bool, ratio float8)"));

  show("insert", await client.query("INSERT INTO p7_pg VALUES (1, 'alpha', true, 1.5)"));
  show("insert2", await client.query("INSERT INTO p7_pg VALUES (2, 'bêta', false, -0.25)"));
  show("insert-null", await client.query("INSERT INTO p7_pg VALUES (3, NULL, NULL, NULL)"));

  showSelect("select-all", await client.query("SELECT id, name, flag, ratio FROM p7_pg ORDER BY id"));
  showSelect("select-empty", await client.query("SELECT id FROM p7_pg WHERE id = 999"));

  // A row wider than one read, so the core has to reassemble a DataRow that
  // arrives in pieces. Only its length is printed: the value itself would
  // dominate the diff without saying anything more.
  const wide = (await client.query("SELECT repeat('z', 70000) AS wide")) as Res;
  const wideRows = wide.rows as Array<{ wide: string }>;
  console.log("wide-len:", wideRows.length === 1 ? wideRows[0].wide.length : -1);

  show("update", await client.query("UPDATE p7_pg SET flag = true WHERE id = 2"));
  show("delete", await client.query("DELETE FROM p7_pg WHERE id = 3"));

  // A statement error must reject and leave the session usable — under the
  // extended protocol each operation carries its own Sync for exactly that.
  let failed = "no";
  try {
    await client.query("SELECT * FROM p7_no_such_table");
  } catch (e) {
    failed = e instanceof Error && e.message.length > 0 ? "yes" : "empty";
  }
  console.log("error-rejected:", failed);
  showSelect("after-error", await client.query("SELECT id FROM p7_pg ORDER BY id"));

  // A transaction on a Client, which pins one connection for its whole life.
  await client.query("BEGIN");
  await client.query("INSERT INTO p7_pg VALUES (4, 'in-tx', true, 4.0)");
  showSelect("in-tx", await client.query("SELECT id FROM p7_pg WHERE id = 4"));
  await client.query("ROLLBACK");
  showSelect("after-rollback", await client.query("SELECT id FROM p7_pg WHERE id = 4"));

  await client.query("BEGIN");
  await client.query("INSERT INTO p7_pg VALUES (5, 'committed', true, 5.0)");
  await client.query("COMMIT");
  showSelect("after-commit", await client.query("SELECT id FROM p7_pg WHERE id = 5"));

  await client.end();

  // The Pool surface, on its own connection.
  const pool = new Pool(config);
  showSelect("pool-select", await pool.query("SELECT id, name FROM p7_pg ORDER BY id"));
  show("pool-drop", await pool.query("DROP TABLE p7_pg"));
  await pool.end();

  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
