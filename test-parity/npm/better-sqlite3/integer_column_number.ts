// An INTEGER column value is a JS Number with one encoding (the raw double):
// Object.is against the literal is true, arithmetic and JSON match node.
import Database from "better-sqlite3";
const db = new Database(":memory:");
db.exec("CREATE TABLE t (id INTEGER, v INTEGER, big INTEGER, neg INTEGER)");
db.prepare("INSERT INTO t VALUES (?, ?, ?, ?)").run(1, 5, 4294967296, -2147483648);
const row: any = db.prepare("SELECT id, v, big, neg FROM t").get();
console.log(JSON.stringify(row));
console.log(row.v * 1.5, row.big + 1, row.neg - 1);
console.log(Object.is(row.v, 5), Object.is(row.id, 1), Object.is(row.neg, -2147483648));
const raw: any = (db.prepare("SELECT v FROM t") as any).raw().get();
console.log(Object.is(raw[0], 5), raw[0] / 2);
