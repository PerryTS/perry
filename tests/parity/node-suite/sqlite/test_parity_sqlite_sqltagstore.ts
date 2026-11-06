import { DatabaseSync } from "node:sqlite";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (e) {
    return (e as any)?.code || (e as Error)?.name || String(e);
  }
}

const db = new DatabaseSync(":memory:");
const store = db.createTagStore();

console.log("createTagStore typeof:", typeof db.createTagStore);
console.log("store methods:", typeof store.all, typeof store.get, typeof store.iterate, typeof store.run, typeof store.clear);
console.log("store constructor:", (store as any).constructor?.name);
console.log("store defaults:", store.capacity, store.size, store.db === db);

const lru = db.createTagStore(2.9);
console.log("lru capacity:", lru.capacity);
console.log("lru initial size:", lru.size);
lru.get`SELECT 1 AS n`;
lru.get`SELECT 2 AS n`;
console.log("lru size two:", lru.size);
lru.get`SELECT 1 AS n`;
lru.get`SELECT 3 AS n`;
console.log("lru size evicted:", lru.size);

db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, amount INTEGER)");
const inserted = store.run`INSERT INTO t (name, amount) VALUES (${"alice"}, ${7})`;
console.log("run result:", inserted.changes, inserted.lastInsertRowid);

const row = store.get`SELECT id, name, amount FROM t WHERE name = ${"alice"}`;
console.log("get row:", `${row.id}:${row.name}:${row.amount}`);
console.log("size after get:", store.size);

store.run`INSERT INTO t (name, amount) VALUES (${"bob"}, ${9})`;
const rows = store.all`SELECT name, amount FROM t ORDER BY id`;
console.log("all rows:", rows.map((r: any) => `${r.name}:${r.amount}`).join("|"));

const iterValues: string[] = [];
for (const item of store.iterate`SELECT name, amount FROM t ORDER BY id`) {
  iterValues.push(`${item.name}:${item.amount}`);
}
console.log("iterate rows:", iterValues.join("|"));

console.log("clear return:", String(store.clear()));
console.log("size after clear:", store.size);

const zero = db.createTagStore(0);
zero.get`SELECT 1 AS n`;
zero.get`SELECT 2 AS n`;
console.log("zero capacity:", zero.capacity, zero.size);

console.log("missing template:", codeOf(() => (store as any).get()));
console.log("string template:", codeOf(() => (store as any).get("SELECT 1")));
console.log("bad template part:", codeOf(() => (store as any).get(["SELECT ", 1], 1)));

db.close();
console.log("closed create:", codeOf(() => db.createTagStore()));
console.log("closed get:", codeOf(() => store.get`SELECT 1 AS n`));
