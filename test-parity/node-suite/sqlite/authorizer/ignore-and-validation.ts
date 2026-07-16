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

db.setAuthorizer(
  (action: number, table: string | null, column: string | null) => {
    if (
      action === constants.SQLITE_READ &&
      table === "data" &&
      column === "value"
    ) {
      return constants.SQLITE_IGNORE;
    }
    return constants.SQLITE_OK;
  },
);
const ignored: any = db.prepare("SELECT id, value FROM data").get();
console.log("ignored read:", ignored.id, String(ignored.value));

db.setAuthorizer((action: number) =>
  action === constants.SQLITE_INSERT
    ? constants.SQLITE_IGNORE
    : constants.SQLITE_OK,
);
db.prepare("INSERT INTO data VALUES (2, 'two')").run();
db.setAuthorizer(null);
console.log(
  "ignored insert:",
  (db.prepare("SELECT count(*) AS n FROM data").get() as any).n,
);

for (const [label, callback] of [
  ["undefined return", () => undefined],
  ["string return", () => "0"],
  ["invalid code", () => 3],
  [
    "throw",
    () => {
      throw new RangeError("authorizer marker");
    },
  ],
] as const) {
  db.setAuthorizer(callback as any);
  probe(label, () => db.prepare("SELECT 1").get());
}
db.setAuthorizer(null);
db.close();
