// `Statement#columns()` and `Statement#safeIntegers()` on better-sqlite3.
//
// Both read as `undefined` before this fixture's fix: the extension served
// run/get/all/raw and the documented data properties, but not these two. A
// caller that does `stmt.columns().map(...)` — which is what ActiveRecord's
// SQLite adapter does to decide whether to read big integers — got
// "Cannot read properties of undefined (reading 'map')".
import Database from "better-sqlite3";

const db = new Database(":memory:");
db.exec("create table trails (id integer primary key, name text, len bigint)");
db.exec("insert into trails (name, len) values ('Ridge', 12)");

const stmt = db.prepare("select id, name, len from trails");

// The five keys better-sqlite3 reports, in the runtime's canonical order.
const cols = stmt.columns();
console.log("count =", cols.length);
console.log("keys =", JSON.stringify(Object.keys(cols[0])));
console.log(
  "names =",
  JSON.stringify(cols.map((c) => c.name)),
);
console.log(
  "types =",
  JSON.stringify(cols.map((c) => c.type)),
);
// The origin of a real table column, and null for an expression.
console.log("table =", JSON.stringify(cols[0].table));
console.log("column =", JSON.stringify(cols[0].column));
const expr = db.prepare("select count(*) as n from trails").columns();
console.log("expr table =", JSON.stringify(expr[0].table));
console.log("expr type =", JSON.stringify(expr[0].type));

// A method read yields the bound method, not undefined.
console.log("typeof columns =", typeof stmt.columns);
console.log("typeof safeIntegers =", typeof stmt.safeIntegers);

// safeIntegers() flips INTEGER columns to BigInt, and chains.
const plain = db.prepare("select id from trails").get() as { id: unknown };
console.log("default typeof id =", typeof plain.id, String(plain.id));

const big = db.prepare("select id from trails");
const chained = big.safeIntegers();
console.log("chains =", chained === big);
const safe = big.get() as { id: unknown };
console.log("safe typeof id =", typeof safe.id, String(safe.id));

// ...and an explicit false turns it back off.
big.safeIntegers(false);
const off = big.get() as { id: unknown };
console.log("off typeof id =", typeof off.id, String(off.id));
