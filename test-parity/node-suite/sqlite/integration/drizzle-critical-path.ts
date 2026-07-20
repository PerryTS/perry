// parity-node-argv: --experimental-sqlite
// The opencode critical path (#6561): its `sqlite.node.ts` uses
// `import { DatabaseSync } from "node:sqlite"` under drizzle-orm's pure-JS
// `node-sqlite` adapter. This mirrors the adapter's call shapes without the
// npm dependency: schema DDL via exec, prepared inserts with named params,
// selects with positional params, Uint8Array blob round-trip, and SQL
// transactions.
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");

db.exec(`
  CREATE TABLE session (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    time_created INTEGER NOT NULL,
    data BLOB
  );
  CREATE INDEX session_time_idx ON session (time_created);
`);

const insert = db.prepare(
  "INSERT INTO session (id, title, version, time_created, data) VALUES ($id, $title, $version, $time, $data)"
);
const blob = new Uint8Array([104, 101, 108, 108, 111]);
const r1 = insert.run({ id: "s1", title: "first", version: 2, time: 1000, data: blob });
const r2 = insert.run({ id: "s2", title: "second", version: 1, time: 2000, data: null });
console.log("insert changes:", r1.changes, r2.changes, "rowid type:", typeof r2.lastInsertRowid);

const byId = db.prepare("SELECT id, title, version, time_created FROM session WHERE id = ?");
console.log("get s1:", JSON.stringify(byId.get("s1")));
console.log("get missing:", String(byId.get("nope")));

const list = db.prepare("SELECT id FROM session ORDER BY time_created DESC").all() as any[];
console.log("ordered ids:", list.map((row) => row.id).join(","));

const stored = db.prepare("SELECT data FROM session WHERE id = ?").get("s1") as any;
console.log(
  "blob round-trip:",
  stored.data instanceof Uint8Array,
  Array.from(stored.data).join(","),
  "text:",
  new TextDecoder().decode(stored.data)
);

const upd = db.prepare("UPDATE session SET title = $title WHERE id = $id").run({ id: "s1", title: "renamed" });
console.log("update changes:", upd.changes, "->", (byId.get("s1") as any).title);

db.exec("BEGIN");
db.prepare("DELETE FROM session WHERE id = ?").run("s2");
console.log("in tx:", db.isTransaction, "count:", (db.prepare("SELECT COUNT(*) n FROM session").get() as any).n);
db.exec("ROLLBACK");
console.log("after rollback count:", (db.prepare("SELECT COUNT(*) n FROM session").get() as any).n);

db.exec("BEGIN");
db.prepare("DELETE FROM session WHERE id = ?").run("s2");
db.exec("COMMIT");
console.log("after commit count:", (db.prepare("SELECT COUNT(*) n FROM session").get() as any).n);

db.close();
console.log("done isOpen:", db.isOpen);
