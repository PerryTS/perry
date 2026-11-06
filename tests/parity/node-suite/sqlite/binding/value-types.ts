// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function bytes(value: any) {
  return Array.from(value as Uint8Array).join(",");
}

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    console.log(label, "OK", value?.changes ?? value);
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:", { readBigInts: true });
db.exec("CREATE TABLE values_table(id INTEGER PRIMARY KEY, value)");
const insert = db.prepare("INSERT INTO values_table(value) VALUES (?)");
probe("null", () => insert.run(null));
probe("text", () => insert.run("hello"));
probe("number", () => insert.run(1.25));
probe("bigint", () => insert.run(9007199254740993n));
probe("buffer", () => insert.run(Buffer.from([1, 2, 255])));
probe("uint8", () => insert.run(new Uint8Array([3, 4])));
probe("dataview", () =>
  insert.run(new DataView(Uint8Array.from([5, 6]).buffer)),
);
probe("empty blob", () => insert.run(new Uint8Array()));

const rows: any[] = db
  .prepare("SELECT value, typeof(value) AS type FROM values_table ORDER BY id")
  .all();
for (const [index, row] of rows.entries()) {
  const value = row.value;
  console.log(
    "row:",
    index + 1,
    row.type,
    typeof value,
    row.type === "blob" ? bytes(value) : String(value),
  );
}

for (const value of [
  undefined,
  true,
  {},
  [],
  Symbol("x"),
  2n ** 63n,
  -(2n ** 63n) - 1n,
]) {
  probe(`invalid ${typeof value}`, () => insert.run(value as any));
}
db.close();
