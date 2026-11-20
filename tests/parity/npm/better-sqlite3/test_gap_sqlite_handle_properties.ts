// better-sqlite3 exposes `source`, `reader`, `readonly`, `busy` on a Statement
// and `name`, `open`, `inTransaction`, `memory`, `readonly` on a Database.
// Perry hands both to JS as handle ids, so a property read has no object to
// find them on: the extension registered handle METHOD dispatch but never the
// property counterpart, and every one of these read `undefined`.
//
// It also broke capturing a method as a value — `const f = stmt.get` — which
// library code does for feature detection (`typeof x.get === "function"`).
//
// Found through ActiveRecord's sqlite3 adapter, which reads `this.stmt.source`
// in its bind path and failed as "Cannot read properties of undefined
// (reading 'split')".
import Database from "better-sqlite3";

const db = new Database(":memory:");
const stmt = db.prepare("select 1 as one");

// the statement still works
console.log("get().one=", (stmt.get() as { one: number }).one);

// Statement data properties
console.log("source=", stmt.source);
console.log("reader=", stmt.reader);
console.log("readonly=", stmt.readonly);
console.log("busy=", stmt.busy);

// Database data properties
console.log("db.name=", db.name);
console.log("db.open=", db.open);
console.log("db.inTransaction=", db.inTransaction);
console.log("db.memory=", db.memory);
console.log("db.readonly=", db.readonly);

// a method read as a value, not called
console.log("typeof stmt.get=", typeof stmt.get);
console.log("typeof stmt.run=", typeof stmt.run);
console.log("typeof db.prepare=", typeof db.prepare);

// `in` agrees with the property read
console.log("'source' in stmt=", "source" in stmt);
console.log("'open' in db=", "open" in db);

// a name the extension does not serve stays unclaimed, so a user property
// written onto the handle still reads back
(stmt as unknown as Record<string, unknown>).userField = 7;
console.log("userField=", (stmt as unknown as Record<string, unknown>).userField);

// after a write, inTransaction still reports correctly
db.exec("create table t (id integer)");
console.log("db.inTransaction after exec=", db.inTransaction);
