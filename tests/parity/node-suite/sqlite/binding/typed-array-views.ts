// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const bytes = Uint8Array.from([1, 2, 3, 4, 5, 6, 7, 8]);
const constructors: [string, new (buffer: ArrayBuffer) => ArrayBufferView][] = [
  ["Int8Array", Int8Array],
  ["Uint8Array", Uint8Array],
  ["Uint8ClampedArray", Uint8ClampedArray],
  ["Int16Array", Int16Array],
  ["Uint16Array", Uint16Array],
  ["Int32Array", Int32Array],
  ["Uint32Array", Uint32Array],
  ["Float32Array", Float32Array],
  ["Float64Array", Float64Array],
  ["BigInt64Array", BigInt64Array],
  ["BigUint64Array", BigUint64Array],
  ["DataView", DataView],
];

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE views(name TEXT PRIMARY KEY, value BLOB)");
const insert = db.prepare("INSERT INTO views VALUES (?, ?)");
const lookup = db.prepare(
  "SELECT value FROM views WHERE name = ? AND value = ?",
);

for (const [name, Constructor] of constructors) {
  try {
    const input = new Constructor(bytes.buffer.slice(0));
    insert.run(name, input);
    const row: any = lookup.get(name, input);
    console.log(
      name,
      "OK",
      row.value instanceof Uint8Array,
      row.value.length,
      Array.from(row.value).join(","),
    );
  } catch (error: any) {
    console.log(name, "THROW", error.name, error.code || "no-code");
  }
}
db.close();
