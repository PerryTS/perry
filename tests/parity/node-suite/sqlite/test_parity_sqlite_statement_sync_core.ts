import * as sqlite from "node:sqlite";
import { DatabaseSync, StatementSync } from "node:sqlite";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (e) {
    return (e as any)?.code || (e as Error)?.name || String(e);
  }
}

function scalar(value: unknown): string {
  if (typeof value === "bigint") return `${value.toString()}n`;
  return String(value);
}

console.log("StatementSync typeof:", typeof StatementSync);
console.log("namespace StatementSync typeof:", typeof sqlite.StatementSync);
console.log("StatementSync call:", codeOf(() => (StatementSync as any)()));
console.log("StatementSync new:", codeOf(() => new (StatementSync as any)()));

const db = new DatabaseSync(":memory:");
console.log("prepare bad options:", codeOf(() => db.prepare("SELECT 1", null as any)));
console.log(
  "prepare bad readBigInts:",
  codeOf(() => db.prepare("SELECT 1", { readBigInts: 1 as any })),
);

const probe = db.prepare("SELECT ? AS a, :b AS b");
console.log("stmt.run typeof:", typeof probe.run);
console.log("stmt.get typeof:", typeof probe.get);
console.log("stmt.all typeof:", typeof probe.all);
console.log("stmt.iterate typeof:", typeof probe.iterate);
console.log("stmt.columns typeof:", typeof probe.columns);
console.log("stmt.setReadBigInts typeof:", typeof probe.setReadBigInts);
console.log("stmt.setReturnArrays typeof:", typeof probe.setReturnArrays);
console.log("stmt.sourceSQL:", probe.sourceSQL);
console.log("stmt.expandedSQL initial:", probe.expandedSQL);
console.log("setter bad arg:", codeOf(() => probe.setReturnArrays(1 as any)));

db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, amount INTEGER)");
const insert = db.prepare("INSERT INTO t (name, amount) VALUES (:name, ?)");
const inserted = insert.run({ name: "alice" }, 7);
console.log("insert changes:", scalar(inserted.changes));
console.log("insert rowid:", scalar(inserted.lastInsertRowid));
const expandedAfterInsert = insert.expandedSQL;
console.log("expanded has alice:", String(expandedAfterInsert.includes("'alice'")));
console.log("expanded has seven:", String(expandedAfterInsert.includes("7")));

const row = db.prepare("SELECT id, name, amount FROM t WHERE name = ?").get("alice");
console.log("row proto null:", String(Object.getPrototypeOf(row) === null));
console.log("row keys:", Object.keys(row).join(","));
console.log("row values:", `${row.id}:${row.name}:${row.amount}`);

const rows = db.prepare("SELECT name, amount FROM t ORDER BY id").all();
console.log("all length:", rows.length);
console.log("all first:", `${rows[0].name}:${rows[0].amount}`);

const iterValues: string[] = [];
for (const item of db.prepare("SELECT name, amount FROM t ORDER BY id").iterate()) {
  iterValues.push(`${item.name}:${item.amount}`);
}
console.log("iterate values:", iterValues.join("|"));

const arrays = db.prepare("SELECT name, amount FROM t ORDER BY id");
arrays.setReturnArrays(true);
const arrayRow = arrays.get();
console.log("array row:", `${Array.isArray(arrayRow)}:${arrayRow[0]}:${arrayRow[1]}`);

const named = db.prepare("SELECT :x AS x, ? AS y");
console.log("named bare:", `${named.get({ x: 2 }, 3).x}:${named.get({ x: 2 }, 3).y}`);
named.setAllowBareNamedParameters(false);
console.log("named bare disabled:", codeOf(() => named.get({ x: 2 }, 3)));
console.log("named prefixed:", `${named.get({ ":x": 4 }, 5).x}:${named.get({ ":x": 4 }, 5).y}`);

const unknown = db.prepare("SELECT :x AS x");
console.log("unknown default:", codeOf(() => unknown.get({ x: 1, z: 2 })));
unknown.setAllowUnknownNamedParameters(true);
console.log("unknown allowed:", unknown.get({ x: 1, z: 2 }).x);

const ambiguous = db.prepare("SELECT :x AS a, @x AS b");
console.log("ambiguous bare:", codeOf(() => ambiguous.get({ ":x": 1, "@x": 2 })));
ambiguous.setAllowBareNamedParameters(false);
const disambiguated = ambiguous.get({ ":x": 1, "@x": 2 });
console.log("ambiguous prefixed:", `${disambiguated.a}:${disambiguated.b}`);

console.log("bind undefined:", codeOf(() => db.prepare("SELECT ?").get(undefined as any)));
console.log("bind too many:", codeOf(() => db.prepare("SELECT :x").get(1)));
console.log("bind big too large:", codeOf(() => db.prepare("SELECT ?").get(1n << 100n)));

const big = db.prepare("SELECT 9007199254740992 AS n");
console.log("unsafe read:", codeOf(() => big.get()));
big.setReadBigInts(true);
const bigRow = big.get();
console.log("big read:", `${typeof bigRow.n}:${scalar(bigRow.n)}`);

const bigInsert = db.prepare("INSERT INTO t (name, amount) VALUES (?, ?)");
bigInsert.setReadBigInts(true);
const bigResult = bigInsert.run("bob", 9);
console.log(
  "big run result:",
  `${typeof bigResult.changes}:${typeof bigResult.lastInsertRowid}:${scalar(bigResult.changes)}`,
);

const cols = db.prepare("SELECT id AS ident, name, 1 AS one FROM t").columns();
console.log("columns length:", cols.length);
console.log("columns keys:", Object.keys(cols[0]).join(","));
console.log(
  "columns first:",
  `${cols[0].column}:${cols[0].database}:${cols[0].name}:${cols[0].table}:${cols[0].type}`,
);
console.log(
  "columns expr:",
  `${cols[2].column}:${cols[2].database}:${cols[2].name}:${cols[2].table}:${cols[2].type}`,
);

const stale = db.prepare("SELECT 1 AS one");
db.close();
console.log("finalized get:", codeOf(() => stale.get()));
