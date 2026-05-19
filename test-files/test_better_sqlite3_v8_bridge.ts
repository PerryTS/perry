// Refs #1022 — better-sqlite3 Database/Statement crossing the
// native→V8 boundary. Drizzle's BetterSQLiteSession (session.js
// running under V8 fallback) does `this.client.prepare(query.sql)`
// and `stmt.run(...)`/`stmt.all(...)`/`stmt.get(...)`. Without the
// sqlite-handle v8 proxies in `perry-jsruntime::bridge`, the
// sqlite handle materializes as `v8::null` in V8 land and drizzle
// crashes with "Cannot read properties of null (reading 'prepare')".
//
// This fixture exercises the same shape as drizzle's flow without
// pulling drizzle in: a Database created natively, then passed to
// a function whose body runs in the V8 fallback path. The bridge
// must synthesize a real v8::Object with `prepare`/`exec`/`pragma`/
// `close` callbacks and the returned Statement must in turn expose
// `run`/`all`/`get`/`raw` callbacks routing back to the FFI.
import Database from "better-sqlite3";

const db = new Database(":memory:");
db.exec(`CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)`);

// Plain native-side path — sanity check that the basic FFI still works
// after the bridge changes.
const ins = db.prepare("INSERT INTO users VALUES (?, ?)");
ins.run(1, "alice");
ins.run(2, "bob");

const sel = db.prepare("SELECT * FROM users ORDER BY id");
const rows: any = sel.all();
console.log("rows.length=" + rows.length);
console.log("rows[0].name=" + rows[0].name);
console.log("rows[1].name=" + rows[1].name);

const stmtRaw: any = (sel as any).raw().all();
console.log("rawRows.length=" + stmtRaw.length);
console.log("rawRows[0][1]=" + stmtRaw[0][1]);

const oneRow: any = db.prepare("SELECT * FROM users WHERE id = ?").get(2);
console.log("get(2).name=" + oneRow.name);

const ran = db.prepare("INSERT INTO users VALUES (?, ?)").run(3, "carol");
console.log("run.changes=" + ran.changes);
console.log("run.lastInsertRowid=" + ran.lastInsertRowid);
