// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    console.log(label, "OK", value?.changes ?? value);
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:");

function register(name: string, directOnly?: boolean) {
  const options: any = {
    start: 0,
    step: (total: number, value: number) => total + value,
    inverse: (total: number, value: number) => total - value,
  };
  if (directOnly !== undefined) options.directOnly = directOnly;
  db.aggregate(name, options);
  db.exec(`
    CREATE TABLE ${name}_data(value INTEGER);
    CREATE TRIGGER ${name}_trigger AFTER INSERT ON ${name}_data BEGIN
      SELECT ${name}(value) OVER () FROM ${name}_data;
    END;
  `);
}

register("default_aggregate");
register("false_aggregate", false);
register("true_aggregate", true);

probe("default trigger", () =>
  db.prepare("INSERT INTO default_aggregate_data VALUES (?)").run(1),
);
probe("false trigger", () =>
  db.prepare("INSERT INTO false_aggregate_data VALUES (?)").run(2),
);
probe("true trigger", () =>
  db.prepare("INSERT INTO true_aggregate_data VALUES (?)").run(3),
);
console.log(
  "counts:",
  (db.prepare("SELECT count(*) AS n FROM default_aggregate_data").get() as any)
    .n,
  (db.prepare("SELECT count(*) AS n FROM false_aggregate_data").get() as any).n,
  (db.prepare("SELECT count(*) AS n FROM true_aggregate_data").get() as any).n,
);
db.close();
