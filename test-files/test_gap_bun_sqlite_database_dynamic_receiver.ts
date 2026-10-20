// A bun:sqlite `Database` reached through an untyped value: a field, a Map,
// an `any` local, a generator resume value captured by closures (opencode's
// SqliteBun layer: `const native = (yield* Sqlite.Native) as Database`, then
// `native.query(q)` inside `run`). `new Database()` boxed its handle id with
// the heap-pointer tag, so the dynamic method call read a GC header at
// address id - 8 and crashed; only statically typed `db.query(...)` worked.
import { Database } from "bun:sqlite";

const db = new Database(":memory:", { readwrite: true, create: true });
db.run("CREATE TABLE t (a INTEGER, b TEXT)");
db.run("INSERT INTO t VALUES (1, 'x'), (2, 'y')");

const anyDb: any = db;
console.log("any", JSON.stringify(anyDb.query("SELECT a FROM t ORDER BY a").all()));

const holder: any = { native: db };
console.log("field", JSON.stringify(holder.native.query("SELECT b FROM t ORDER BY a").values()));

const services = new Map<string, unknown>([["native", db]]);
const fromMap = services.get("native") as Database;
console.log("map", JSON.stringify(fromMap.query("SELECT COUNT(*) AS n FROM t").get()));

function* layer(): Generator<string, (q: string, params?: unknown[]) => unknown[], unknown> {
  const native = (yield "native") as Database;
  const run = (q: string, params: ReadonlyArray<unknown> = []) => {
    const statement: any = native.query(q);
    statement.safeIntegers(false);
    return statement.all(...(params as any[])) ?? [];
  };
  return run;
}
const g = layer();
console.log("yield", g.next().value);
const run = g.next(services.get("native")).value as (q: string, params?: unknown[]) => unknown[];
console.log("run", JSON.stringify(run("SELECT a, b FROM t WHERE a > ? ORDER BY a", [0])));
console.log("run2", JSON.stringify(run("SELECT b FROM t WHERE a = ?", [2])));

console.log("same", holder.native === db, typeof holder.native);
anyDb.close();
console.log("closed");
