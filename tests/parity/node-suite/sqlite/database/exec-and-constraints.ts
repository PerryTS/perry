// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "OK", value === undefined ? "undefined" : String(value));
  } catch (error: any) {
    console.log(label, "THROW", error?.name, error?.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:");
probe("exec batch", () =>
  db.exec(`
    CREATE TABLE parent(id INTEGER PRIMARY KEY);
    CREATE TABLE child(parent_id INTEGER REFERENCES parent(id));
    INSERT INTO parent VALUES (1);
    INSERT INTO child VALUES (1);
  `),
);
console.log("counts:", db.prepare("SELECT count(*) AS n FROM parent").get().n);
probe("foreign key", () => db.exec("INSERT INTO child VALUES (99)"));
probe("syntax", () => db.exec("SELEC 1"));
for (const value of [undefined, null, 1, true, {}, []]) {
  probe(`exec ${value === null ? "null" : typeof value}`, () =>
    db.exec(value as any),
  );
}
db.close();

const noForeignKeys = new DatabaseSync(":memory:", {
  enableForeignKeyConstraints: false,
});
noForeignKeys.exec(`
  CREATE TABLE parent(id INTEGER PRIMARY KEY);
  CREATE TABLE child(parent_id INTEGER REFERENCES parent(id));
  INSERT INTO child VALUES (99);
`);
console.log(
  "foreign keys disabled:",
  noForeignKeys.prepare("SELECT count(*) AS n FROM child").get().n,
);
noForeignKeys.close();
