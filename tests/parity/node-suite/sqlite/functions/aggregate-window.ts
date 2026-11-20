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

const db = new DatabaseSync(":memory:");
db.exec(
  "CREATE TABLE data(id INTEGER, value INTEGER); INSERT INTO data VALUES (1, 4), (2, 5), (3, 3)",
);
let results = 0;
db.aggregate("moving_sum", {
  start: 0,
  step: (total: number, value: number) => total + value,
  inverse: (total: number, value: number) => total - value,
  result: (total: number) => {
    results++;
    return total;
  },
});
const rows: any[] = db
  .prepare(
    `
    SELECT id, moving_sum(value) OVER (
      ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING
    ) AS value
    FROM data ORDER BY id
  `,
  )
  .all();
console.log(
  "window:",
  rows.map((row) => `${row.id}:${row.value}`).join(","),
  results,
);

db.aggregate("aggregate_only", {
  start: 0,
  step: (total: number, value: number) => total + value,
});
probe("missing inverse", () =>
  db.prepare("SELECT aggregate_only(value) OVER () AS value FROM data").get(),
);

db.aggregate("fixed_arity", {
  start: 0,
  step: (total: number, first: number, second: number) =>
    total + first + second,
});
probe("fixed valid", () =>
  db.prepare("SELECT fixed_arity(1, 2) AS value").get(),
);
probe("fixed invalid", () =>
  db.prepare("SELECT fixed_arity(1) AS value").get(),
);
db.close();
