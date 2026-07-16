// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    console.log(label, "OK", value?.value ?? value);
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const closed = new DatabaseSync(":memory:", { open: false });
probe("create closed", () => closed.createTagStore(1));

const db = new DatabaseSync(":memory:");
const sql: any = db.createTagStore(2);
for (const [label, call] of [
  ["get string", () => sql.get("SELECT 1")],
  ["all object", () => sql.all({})],
  ["run number", () => sql.run(1)],
  ["iterate null", () => sql.iterate(null)],
] as const) {
  probe(label, call);
}

probe("sql error", () => sql.get`SELECT * FROM missing_table`);
console.log(
  "before close:",
  (sql.get`SELECT ${1} AS value` as any).value,
  sql.size,
);
db.close();
probe("get after close", () => sql.get`SELECT 1 AS value`);
probe("all after close", () => sql.all`SELECT 1 AS value`);
probe("run after close", () => sql.run`SELECT 1`);
probe("iterate after close", () => sql.iterate`SELECT 1 AS value`);
console.log("closed db identity:", sql.db === db, db.isOpen);
