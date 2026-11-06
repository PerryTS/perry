// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function show(label: string, row: any) {
  if (Array.isArray(row)) {
    console.log(
      label,
      "array",
      row.map((value) => `${typeof value}:${String(value)}`).join(","),
    );
  } else {
    console.log(
      label,
      "object",
      Object.keys(row)
        .map((key) => `${key}=${typeof row[key]}:${String(row[key])}`)
        .join(","),
    );
  }
}

function probe(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", String(fn()));
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:");
db.exec(
  "CREATE TABLE data(id INTEGER, big INTEGER); INSERT INTO data VALUES (1, 9007199254740992)",
);
const statement = db.prepare("SELECT id, big FROM data");
probe("number overflow", () => statement.get());
statement.setReadBigInts(true);
show("bigints", statement.get());
statement.setReturnArrays(true);
show("arrays and bigints", statement.get());
statement.setReadBigInts(false);
probe("array overflow", () => statement.get());

const configured = db.prepare("SELECT 2 AS id, 42 AS value", {
  readBigInts: true,
  returnArrays: true,
});
show("prepare options", configured.get());
configured.setReturnArrays(false);
configured.setReadBigInts(false);
show("overridden", configured.get());

for (const [label, call] of [
  ["read", () => statement.setReadBigInts(undefined as any)],
  ["arrays", () => statement.setReturnArrays(1 as any)],
  ["bare", () => statement.setAllowBareNamedParameters("true" as any)],
  ["unknown", () => statement.setAllowUnknownNamedParameters(null as any)],
] as const) {
  probe(`invalid ${label}`, call);
}
db.close();
