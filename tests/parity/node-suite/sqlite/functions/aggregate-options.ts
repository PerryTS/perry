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

const db = new DatabaseSync(":memory:", { readBigInts: true });
db.exec(
  "CREATE TABLE data(value INTEGER); INSERT INTO data VALUES (1), (2), (3)",
);
db.aggregate("big_sum", {
  start: 0n,
  useBigIntArguments: true,
  step: (total: bigint, value: bigint) => total + value,
});
const big: any = db.prepare("SELECT big_sum(value) AS value FROM data").get();
console.log("bigint:", typeof big.value, String(big.value));

for (const [label, options] of [
  ["missing start", { step: () => 0 }],
  ["missing step", { start: 0 }],
  ["bigint option", { start: 0, step: () => 0, useBigIntArguments: "true" }],
  ["varargs option", { start: 0, step: () => 0, varargs: 1 }],
  ["direct option", { start: 0, step: () => 0, directOnly: null }],
  ["inverse option", { start: 0, step: () => 0, inverse: 1 }],
  ["result option", { start: 0, step: () => 0, result: 1 }],
] as const) {
  probe(label, () =>
    db.aggregate(`invalid_${label.replace(" ", "_")}`, options as any),
  );
}

db.aggregate("start_error", {
  start: () => {
    throw new RangeError("start marker");
  },
  step: () => 0,
});
probe("start error", () => db.prepare("SELECT start_error() AS value").get());

db.aggregate("step_error", {
  start: 0,
  step: () => {
    throw new RangeError("step marker");
  },
});
probe("step error", () => db.prepare("SELECT step_error() AS value").get());

db.aggregate("result_error", {
  start: 0,
  step: (total: number) => total,
  result: () => {
    throw new RangeError("result marker");
  },
});
probe("result error", () => db.prepare("SELECT result_error() AS value").get());
db.close();
