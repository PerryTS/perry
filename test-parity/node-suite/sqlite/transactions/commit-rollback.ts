// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE events(value TEXT)");
console.log("initial:", db.isTransaction);

db.exec("BEGIN");
console.log("after begin:", db.isTransaction);
db.prepare("INSERT INTO events VALUES (?)").run("committed");
db.exec("COMMIT");
console.log(
  "after commit:",
  db.isTransaction,
  db.prepare("SELECT count(*) AS n FROM events").get().n,
);

db.exec("BEGIN IMMEDIATE");
db.prepare("INSERT INTO events VALUES (?)").run("rolled back");
console.log("during rollback:", db.isTransaction);
db.exec("ROLLBACK");
console.log(
  "after rollback:",
  db.isTransaction,
  db.prepare("SELECT count(*) AS n FROM events").get().n,
);

db.exec("BEGIN; SAVEPOINT nested");
db.prepare("INSERT INTO events VALUES (?)").run("savepoint");
db.exec("ROLLBACK TO nested; RELEASE nested; COMMIT");
console.log(
  "after savepoint:",
  db.isTransaction,
  db.prepare("SELECT count(*) AS n FROM events").get().n,
);
db.close();
