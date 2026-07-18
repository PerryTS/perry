// parity-node-argv: --experimental-sqlite
// node:sqlite error-shape conformance (#6561): ERR_SQLITE_ERROR errors carry
// `errcode` (extended result code) and `errstr` (sqlite3_errstr text) own
// properties in addition to `code`/`message`.
import { DatabaseSync } from "node:sqlite";

function shape(error) {
  return [
    `name=${error.name}`,
    `code=${error.code}`,
    `errcode=${error.errcode} (${typeof error.errcode})`,
    `errstr=${error.errstr}`,
    `msg=${String(error.message).split("\n")[0]}`,
  ].join(" | ");
}

function reportThrow(label, fn) {
  try {
    fn();
    console.log(label, "NO-THROW");
  } catch (error) {
    console.log(label, shape(error));
  }
}

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE t(a UNIQUE); INSERT INTO t VALUES (1);");
console.log(
  "exec multi count:",
  (db.prepare("SELECT COUNT(*) AS n FROM t").get() as any).n
);

reportThrow("syntax error", () => db.exec("NOT VALID SQL"));
reportThrow("prepare missing table", () => db.prepare("SELECT * FROM missing_table"));
reportThrow("unique violation", () => db.prepare("INSERT INTO t VALUES (?)").run(1));

const fkdb = new DatabaseSync(":memory:");
fkdb.exec("CREATE TABLE p(id INTEGER PRIMARY KEY); CREATE TABLE c(pid REFERENCES p(id));");
reportThrow("fk violation (default on)", () => fkdb.exec("INSERT INTO c VALUES (99)"));
fkdb.close();

const nofk = new DatabaseSync(":memory:", { enableForeignKeyConstraints: false });
nofk.exec("CREATE TABLE p(id INTEGER PRIMARY KEY); CREATE TABLE c(pid REFERENCES p(id));");
reportThrow("fk violation (disabled)", () => nofk.exec("INSERT INTO c VALUES (99)"));
nofk.close();

db.close();
