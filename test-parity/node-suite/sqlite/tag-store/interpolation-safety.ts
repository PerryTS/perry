// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE data(value TEXT)");
const sql = db.createTagStore(5);
const payload = "x'); DROP TABLE data; --";
console.log(
  "insert:",
  String(sql.run`INSERT INTO data(value) VALUES (${payload})`.changes),
);
console.log("stored:", (sql.get`SELECT value FROM data` as any).value);
console.log(
  "table intact:",
  (sql.get`SELECT count(*) AS n FROM data` as any).n,
);

function lookup(value: string) {
  return sql.get`SELECT value FROM data WHERE value = ${value}` as any;
}
console.log("cached first:", lookup(payload).value, sql.size);
console.log("cached miss:", String(lookup("missing")), sql.size);
console.log("cached again:", lookup(payload).value, sql.size);
db.close();
