// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", String(fn()));
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:");
let calls = 0;
db.function("double_value", { deterministic: true }, (value: number) => {
  calls++;
  return value * 2;
});
db.function("join_values", { varargs: true }, (...values: any[]) =>
  values.join("|"),
);
db.function("blob_tail", (value: Uint8Array) => value[value.length - 1]);

console.log(
  "double:",
  db.prepare("SELECT double_value(21) AS value").get().value,
  calls,
);
console.log(
  "varargs:",
  db.prepare("SELECT join_values('a', 2, NULL) AS value").get().value,
);
console.log(
  "blob:",
  db.prepare("SELECT blob_tail(?) AS value").get(Uint8Array.from([3, 7, 9]))
    .value,
);
probe("wrong arity", () => db.prepare("SELECT double_value(1, 2)").get());

db.function("throws_value", () => {
  throw new RangeError("callback marker");
});
probe("callback throw", () => db.prepare("SELECT throws_value()").get());

for (const [label, call] of [
  ["name", () => db.function(1 as any, () => 1)],
  ["callback", () => db.function("bad_callback", 1 as any)],
  ["options", () => db.function("bad_options", null as any, () => 1)],
] as const) {
  probe(`invalid ${label}`, call);
}
db.close();
