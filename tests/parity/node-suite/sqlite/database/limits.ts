// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", String(fn()));
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const names = [
  "length",
  "sqlLength",
  "column",
  "exprDepth",
  "compoundSelect",
  "vdbeOp",
  "functionArg",
  "attach",
  "likePatternLength",
  "variableNumber",
  "triggerDepth",
] as const;

const db = new DatabaseSync(":memory:");
console.log("keys:", Object.keys(db.limits).join(","));
console.log(
  "types:",
  names.every((name) => typeof db.limits[name] === "number"),
);
const originalLength = db.limits.length;
db.limits.length = 100000;
console.log("set length:", db.limits.length);
db.limits.length = Infinity;
console.log("reset length:", db.limits.length === originalLength);
for (const value of [-1, 1.5, NaN, -Infinity, "1", null]) {
  probe(
    `set ${String(value)}`,
    () => ((db.limits.length = value as any), db.limits.length),
  );
}
db.close();
probe("get closed", () => db.limits.length);
probe("set closed", () => ((db.limits.length = 10), db.limits.length));

const limited = new DatabaseSync(":memory:", {
  limits: { column: 3, variableNumber: 2, compoundSelect: 1 },
});
console.log(
  "constructor limits:",
  limited.limits.column,
  limited.limits.variableNumber,
  limited.limits.compoundSelect,
);
probe("column enforced", () =>
  limited.exec("CREATE TABLE too_wide(a, b, c, d)"),
);
probe("variable enforced", () => limited.prepare("SELECT ?, ?, ?"));
probe("compound enforced", () => limited.exec("SELECT 1 UNION SELECT 2"));
limited.close();

for (const limits of [null, 1, "limits", { length: -1 }, { length: 1.5 }]) {
  probe(`constructor ${limits === null ? "null" : typeof limits}`, () => {
    const invalid = new DatabaseSync(":memory:", { limits: limits as any });
    invalid.close();
  });
}
