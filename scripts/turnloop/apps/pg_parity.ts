// P7 acceptance: the `pg` surface, run identically on Perry and on Node 26.5.1
// with the real npm `pg`, against the same PostgreSQL server.
//
// The server this was written against authenticates with **scram-sha-256**, so
// a passing run is also the only evidence that the host-side SCRAM handshake
// works: the sans-I/O core asks for a `ScramSha256` because its constructor
// reads entropy, and Perry builds it in the binding.
//
// Deliberately avoided, because Perry and node-postgres disagree about them
// *independently of this migration*: `int8` (Perry returns a number, pg returns
// a decimal string), `numeric` (Perry now returns a number where sqlx returned
// null and pg returns a string), and the date/time and json families (Perry
// returns null for all of them under either transport). Printing those would
// make this file assert those gaps rather than the transport.
//
//   PGHOST=127.0.0.1 PGPORT=55432 PGUSER=perry PGPASSWORD=perry_test PGDATABASE=perry_test
//
// parity-skip: requires a live PostgreSQL fixture
import pg from "pg";

const { Client, Pool } = pg;

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

function show(label: string, res: { rows: unknown[]; rowCount: number | null; command: string }) {
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

  show("select-all", await client.query("SELECT id, name, flag, ratio FROM p7_pg ORDER BY id"));
  show("select-empty", await client.query("SELECT id FROM p7_pg WHERE id = 999"));

  // Parameterized: the extended protocol, which is a different message sequence
  // from the simple one and the place a transport bug shows up as a hang.
  show("select-param", await client.query("SELECT name FROM p7_pg WHERE id = $1", [2]));
  show("select-param-text", await client.query("SELECT id FROM p7_pg WHERE name = $1", ["alpha"]));

  // A row wider than one read, so the core has to reassemble a DataRow that
  // arrives in pieces.
  show("wide", await client.query("SELECT repeat('z', 70000) AS wide"));

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
  show("after-error", await client.query("SELECT id FROM p7_pg ORDER BY id"));

  // A transaction on a Client, which pins one connection for its whole life.
  await client.query("BEGIN");
  await client.query("INSERT INTO p7_pg VALUES (4, 'in-tx', true, 4.0)");
  show("in-tx", await client.query("SELECT id FROM p7_pg WHERE id = 4"));
  await client.query("ROLLBACK");
  show("after-rollback", await client.query("SELECT id FROM p7_pg WHERE id = 4"));

  await client.query("BEGIN");
  await client.query("INSERT INTO p7_pg VALUES (5, 'committed', true, 5.0)");
  await client.query("COMMIT");
  show("after-commit", await client.query("SELECT id FROM p7_pg WHERE id = 5"));

  await client.end();

  // The Pool surface, on its own connection.
  const pool = new Pool(config);
  show("pool-select", await pool.query("SELECT id, name FROM p7_pg ORDER BY id"));
  show("pool-drop", await pool.query("DROP TABLE p7_pg"));
  await pool.end();

  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
