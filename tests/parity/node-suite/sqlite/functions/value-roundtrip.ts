// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    if (
      value &&
      typeof value === "object" &&
      Object.prototype.hasOwnProperty.call(value, "value")
    ) {
      console.log(label, "OK", `${typeof value.value}:${String(value.value)}`);
    } else {
      console.log(label, "OK", String(value));
    }
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:", { readBigInts: true });
const seen: string[] = [];
db.function("describe_value", (value: any) => {
  seen.push(
    value instanceof Uint8Array
      ? `blob:${Array.from(value).join(",")}`
      : `${typeof value}:${String(value)}`,
  );
  return value;
});

const statement = db.prepare("SELECT describe_value(?) AS value");
for (const [label, value] of [
  ["null", null],
  ["integer", 42],
  ["real", 1.25],
  ["text", "hello"],
  ["blob", Uint8Array.from([7, 8])],
] as const) {
  const row: any = statement.get(value);
  console.log(
    label,
    typeof row.value,
    row.value instanceof Uint8Array
      ? Array.from(row.value).join(",")
      : String(row.value),
  );
}

db.function(
  "describe_bigint",
  { useBigIntArguments: true },
  (value: bigint) => {
    seen.push(`bigint:${String(value)}`);
    return value;
  },
);
const big: any = db
  .prepare("SELECT describe_bigint(?) AS value")
  .get(9007199254740993n);
console.log("bigint", typeof big.value, String(big.value));
console.log("seen:", seen.join("|"));

for (const [label, value] of [
  ["undefined", undefined],
  ["boolean", true],
  ["object", {}],
  ["array", []],
  ["symbol", Symbol("value")],
] as const) {
  db.function(`return_${label}`, () => value as any);
  probe(`return ${label}`, () =>
    db.prepare(`SELECT return_${label}() AS value`).get(),
  );
}
db.close();
