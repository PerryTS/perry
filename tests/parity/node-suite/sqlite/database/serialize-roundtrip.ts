// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    console.log(
      label,
      "OK",
      value instanceof Uint8Array ? `bytes:${value.length > 0}` : String(value),
    );
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const source = new DatabaseSync(":memory:");
console.log(
  "surface:",
  typeof (source as any).serialize,
  typeof (source as any).deserialize,
);
source.exec("CREATE TABLE data(id INTEGER PRIMARY KEY, text TEXT, blob BLOB)");
source
  .prepare("INSERT INTO data VALUES (?, ?, ?)")
  .run(1, "one", Uint8Array.from([1, 2, 3]));

let image: Uint8Array | undefined;
try {
  image = (source as any).serialize();
  const header = new TextDecoder().decode(image.slice(0, 15));
  console.log(
    "serialized:",
    image instanceof Uint8Array,
    image.length > 0,
    header,
  );
} catch (error: any) {
  console.log("serialized: THROW", error.name, error.code || "no-code");
}

if (image) {
  const target = new DatabaseSync(":memory:");
  target.exec("CREATE TABLE replaced(value TEXT)");
  probe("deserialize", () => (target as any).deserialize(image));
  const row: any = target.prepare("SELECT id, text, blob FROM data").get();
  console.log("roundtrip:", row.id, row.text, Array.from(row.blob).join(","));
  target.close();
}

probe("serialize dbName", () => (source as any).serialize("main"));
probe("serialize bad dbName", () => (source as any).serialize(1));
source.close();
probe("serialize closed", () => (source as any).serialize());

const validation = new DatabaseSync(":memory:");
for (const value of [undefined, null, "image", {}, [], new Uint8Array()]) {
  probe(
    `deserialize ${value === null ? "null" : Array.isArray(value) ? "array" : typeof value}`,
    () => (validation as any).deserialize(value),
  );
}
validation.close();
