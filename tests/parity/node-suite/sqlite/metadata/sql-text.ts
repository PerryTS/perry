// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE data(id INTEGER PRIMARY KEY, name TEXT, payload BLOB)");
const sql = "INSERT INTO data(id, name, payload) VALUES ($id, ?, ?)";
const statement = db.prepare(sql);
console.log("source before:", statement.sourceSQL);
console.log("expanded before:", statement.expandedSQL);
statement.run({ id: 7 }, "O'Reilly", Uint8Array.from([0, 1, 255]));
console.log("source after:", statement.sourceSQL);
console.log("expanded after:", statement.expandedSQL);
console.log("source stable:", statement.sourceSQL === sql);
db.close();
