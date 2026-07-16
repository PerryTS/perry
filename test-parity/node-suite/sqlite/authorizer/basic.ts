// parity-node-argv: --experimental-sqlite
import { constants, DatabaseSync } from "node:sqlite";

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
  "CREATE TABLE data(id INTEGER, value TEXT); INSERT INTO data VALUES (1, 'one')",
);
const calls: string[] = [];
db.setAuthorizer(
  (action: number, first: string | null, second: string | null) => {
    calls.push(`${action}:${String(first)}:${String(second)}`);
    return constants.SQLITE_OK;
  },
);
console.log(
  "allowed:",
  db.prepare("SELECT value FROM data WHERE id = 1").get().value,
  calls.length > 0,
);
console.log(
  "read observed:",
  calls.some((call) => call.startsWith(`${constants.SQLITE_READ}:data:`)),
);

db.setAuthorizer(() => constants.SQLITE_DENY);
probe("denied", () => db.exec("SELECT 1"));
db.setAuthorizer(null);
probe("cleared", () => db.prepare("SELECT 2 AS value").get());

for (const value of [undefined, 1, "callback", {}, []]) {
  probe(
    `invalid ${value === undefined ? "undefined" : Array.isArray(value) ? "array" : typeof value}`,
    () => db.setAuthorizer(value as any),
  );
}
db.close();
