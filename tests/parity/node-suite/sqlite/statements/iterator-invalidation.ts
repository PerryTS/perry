// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function next(label: string, iterator: Iterator<any>) {
  try {
    const value = iterator.next();
    console.log(label, "OK", value.done, value.value?.value ?? value.value);
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:");
db.exec(
  "CREATE TABLE data(value INTEGER); INSERT INTO data VALUES (1), (2), (3)",
);
const statement = db.prepare("SELECT value FROM data ORDER BY value");

let iterator = statement.iterate();
next("get before", iterator);
statement.get();
next("get invalidates", iterator);

iterator = statement.iterate();
next("all before", iterator);
statement.all();
next("all invalidates", iterator);

iterator = statement.iterate();
next("run before", iterator);
statement.run();
next("run invalidates", iterator);

iterator = statement.iterate();
next("iterate before", iterator);
const replacement = statement.iterate();
next("iterate invalidates", iterator);
next("replacement works", replacement);

const other = db.prepare("SELECT value FROM data ORDER BY value DESC");
iterator = statement.iterate();
next("other before", iterator);
other.get();
next("other preserves", iterator);
db.close();
