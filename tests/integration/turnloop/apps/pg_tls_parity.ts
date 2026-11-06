// P12 acceptance: the `pg` surface over **TLS**, run identically on Perry and
// on Node 26.5.1 with the real npm `pg`, against the same TLS-enabled
// PostgreSQL server.
//
// The server this is written against has `ssl = on` and authenticates with
// **scram-sha-256**. With TLS up, PostgreSQL advertises both
// `SCRAM-SHA-256-PLUS` and `SCRAM-SHA-256`, and the client picks. Perry picks
// PLUS whenever it can derive RFC 5929 `tls-server-end-point` data from the
// verified leaf, and the server then RE-COMPUTES that digest from its own
// certificate and compares it inside the SASL exchange — so a run that
// authenticates at all is the server's own verdict on the binding. Which
// mechanism was used is not printed here on purpose: it is not part of the JS
// surface, Node's `pg` may legitimately choose the other one, and printing it
// would make this file assert an implementation detail instead of parity.
// `PERRY_DB_TURNLOOP_DIAG=1` prints it on stderr for the run that needs to say.
//
// The same divergences `pg_parity.ts` documents are avoided here for the same
// reasons — this file is about the transport, not about type mapping.
//
//   PGHOST=127.0.0.1 PGPORT=55432 PGUSER=perry PGPASSWORD=perrypw
//   PGDATABASE=postgres PGSSLROOTCERT=/srv/.../tls/ca.crt
//
// parity-skip: requires a live TLS-enabled PostgreSQL fixture
import { Client } from "pg";
import { readFileSync } from "fs";

const ca = readFileSync(process.env.PGSSLROOTCERT ?? "/dev/null", "utf8");

// The first five keys are read POSITIONALLY by Perry's `parse_pg_config`, so
// their order is load-bearing on Perry and irrelevant on Node. `ssl` is read by
// NAME, which is why it can sit at the end.
const config = {
  host: process.env.PGHOST ?? "127.0.0.1",
  port: Number(process.env.PGPORT ?? "5432"),
  user: process.env.PGUSER ?? "postgres",
  password: process.env.PGPASSWORD ?? "",
  database: process.env.PGDATABASE ?? "postgres",
  ssl: { ca, rejectUnauthorized: true },
};

type Res = { rows: unknown[]; rowCount: number | null; command: string };

function show(label: string, res: Res) {
  console.log(`${label}: command=${res.command} rows=${JSON.stringify(res.rows)}`);
}

function showSelect(label: string, res: Res) {
  console.log(`${label}: command=${res.command} rowCount=${res.rowCount} rows=${JSON.stringify(res.rows)}`);
}

async function main(): Promise<void> {
  const client = new Client(config);
  await client.connect();

  // The server's own answer to "is this connection encrypted". Only the
  // boolean: `version` and `cipher` are the TLS library's choice, and Perry
  // (rustls) and Node (OpenSSL) may legitimately pick different suites.
  showSelect(
    "ssl",
    (await client.query(
      "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
    )) as Res,
  );

  await client.query("DROP TABLE IF EXISTS p12_pg_tls");
  show("create", await client.query("CREATE TABLE p12_pg_tls (id int4, name text)"));
  show("insert", await client.query("INSERT INTO p12_pg_tls VALUES (1, 'alpha')"));
  show("insert2", await client.query("INSERT INTO p12_pg_tls VALUES (2, 'bêta')"));
  showSelect("select-all", await client.query("SELECT id, name FROM p12_pg_tls ORDER BY id"));

  // A row wider than one TLS record (16 KiB) AND wider than one socket read, so
  // the session has to reassemble across both boundaries. Only the length is
  // printed; the value would dominate the diff without saying more.
  const wide = (await client.query("SELECT repeat('z', 70000) AS wide")) as Res;
  const wideRows = wide.rows as Array<{ wide: string }>;
  console.log("wide-len:", wideRows.length === 1 ? wideRows[0].wide.length : -1);

  // A statement error must reject and leave the encrypted session usable.
  let failed = "no";
  try {
    await client.query("SELECT * FROM p12_no_such_table");
  } catch (e) {
    failed = e instanceof Error && e.message.length > 0 ? "yes" : "empty";
  }
  console.log("error-rejected:", failed);
  showSelect("after-error", await client.query("SELECT id FROM p12_pg_tls ORDER BY id"));

  show("drop", await client.query("DROP TABLE p12_pg_tls"));
  await client.end();

  // The negative control, and the reason it is here: a TLS client that does not
  // actually verify anything passes every test above. This connects with
  // `rejectUnauthorized` on and NO trust material for the server's private CA,
  // so a connection that succeeds means verification is a no-op. Only the
  // boolean is printed — rustls and OpenSSL word the refusal differently.
  const unverified = new Client({
    host: config.host,
    port: config.port,
    user: config.user,
    password: config.password,
    database: config.database,
    ssl: { rejectUnauthorized: true },
  });
  let refused = "no";
  try {
    await unverified.connect();
    await unverified.end();
  } catch {
    refused = "yes";
  }
  console.log("untrusted-ca-refused:", refused);

  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
