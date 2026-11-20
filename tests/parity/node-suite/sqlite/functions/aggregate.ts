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
db.exec(
  "CREATE TABLE data(value INTEGER); INSERT INTO data VALUES (1), (2), (3)",
);

db.aggregate("sum_plus", {
  start: 10,
  step: (total: number, value: number) => total + value,
  result: (total: number) => total * 2,
});
console.log(
  "value start:",
  db.prepare("SELECT sum_plus(value) AS value FROM data").get().value,
);

let starts = 0;
db.aggregate("joined", {
  start: () => {
    starts++;
    return "";
  },
  step: (text: string, value: number) => text + value,
});
console.log(
  "function start:",
  db.prepare("SELECT joined(value) AS value FROM data").get().value,
  starts,
);

db.aggregate("var_count", {
  start: 0,
  varargs: true,
  step: (count: number, ...values: any[]) => count + values.length,
});
console.log(
  "varargs:",
  db.prepare("SELECT var_count(value, value + 1) AS value FROM data").get()
    .value,
);

for (const [label, call] of [
  ["name", () => db.aggregate(1 as any, { step: () => 0 })],
  ["options", () => db.aggregate("bad_options", null as any)],
  ["step", () => db.aggregate("bad_step", { step: 1 as any })],
] as const) {
  probe(`invalid ${label}`, call);
}
db.close();
