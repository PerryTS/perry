import * as sqlite from "node:sqlite";
import { DatabaseSync } from "node:sqlite";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (e) {
    return (e as any)?.code || (e as Error)?.name || String(e);
  }
}

console.log("DatabaseSync typeof:", typeof DatabaseSync);
console.log("namespace DatabaseSync typeof:", typeof sqlite.DatabaseSync);

console.log("call no new:", codeOf(() => (DatabaseSync as any)(":memory:")));
console.log("missing path:", codeOf(() => new (DatabaseSync as any)()));
console.log("bad options:", codeOf(() => new DatabaseSync(":memory:", null as any)));
console.log("bad open option:", codeOf(() => new DatabaseSync(":memory:", { open: 1 as any })));
console.log(
  "bad limit option:",
  codeOf(() => new DatabaseSync(":memory:", { limits: { length: -1 } } as any)),
);

const db = new DatabaseSync(":memory:");
console.log("isOpen initial:", String(db.isOpen));
console.log("method open typeof:", typeof db.open);
console.log("method close typeof:", typeof db.close);
console.log("method exec typeof:", typeof db.exec);
console.log("method prepare typeof:", typeof db.prepare);
console.log("method location typeof:", typeof db.location);
console.log("dispose typeof:", typeof (db as any)[Symbol.dispose]);
console.log("location memory:", String(db.location() === null));
console.log("limits typeof:", typeof db.limits);
console.log("limits stable:", String(db.limits === db.limits));
console.log("limits length typeof:", typeof db.limits.length);

console.log("isTransaction initial:", String(db.isTransaction));
db.exec("BEGIN");
console.log("isTransaction begin:", String(db.isTransaction));
db.exec("ROLLBACK");
console.log("isTransaction rollback:", String(db.isTransaction));

db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)");
const insert = db.prepare("INSERT INTO t (name) VALUES (?)");
const inserted = insert.run("alice");
console.log("insert changes:", inserted.changes);
console.log("insert rowid:", inserted.lastInsertRowid);
const row = db.prepare("SELECT id, name FROM t WHERE name = ?").get("alice");
console.log("select id:", row.id);
console.log("select name:", row.name);

console.log("bad exec sql:", codeOf(() => db.exec(1 as any)));
console.log("sqlite exec error:", codeOf(() => db.exec("CREATE TABLE")));
console.log("bad prepare sql:", codeOf(() => db.prepare(1 as any)));
console.log("bad location db:", codeOf(() => db.location(null as any)));

db.close();
console.log("isOpen closed:", String(db.isOpen));
console.log("closed exec:", codeOf(() => db.exec("SELECT 1")));
console.log("close duplicate:", codeOf(() => db.close()));
console.log("dispose closed:", String((db as any)[Symbol.dispose]()));

const lazy = new sqlite.DatabaseSync(":memory:", { open: false });
console.log("lazy isOpen initial:", String(lazy.isOpen));
console.log("lazy close before open:", codeOf(() => lazy.close()));
console.log("lazy open result:", String(lazy.open()));
console.log("lazy isOpen open:", String(lazy.isOpen));
console.log("lazy open duplicate:", codeOf(() => lazy.open()));
console.log("lazy dispose open:", String((lazy as any)[Symbol.dispose]()));
console.log("lazy isOpen disposed:", String(lazy.isOpen));
console.log("lazy dispose duplicate:", String((lazy as any)[Symbol.dispose]()));
