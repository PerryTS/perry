// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
db.exec(`
  CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT);
  INSERT INTO items(name) VALUES ('a'), ('b'), ('c');
`);

const statement = db.prepare("SELECT id, name FROM items ORDER BY id");
const iterator: any = statement.iterate();
console.log("shape:", typeof iterator.next, typeof iterator[Symbol.iterator]);
console.log("self iterable:", iterator[Symbol.iterator]() === iterator);
const first = iterator.next();
console.log("first:", first.done, first.value.id, first.value.name);
const rest: string[] = [];
for (const row of iterator) rest.push(`${row.id}:${row.name}`);
console.log("rest:", rest.join(","));
console.log("done:", JSON.stringify(iterator.next()));

const secondPass = [...statement.iterate()].map((row: any) => row.name);
console.log("second pass:", secondPass.join(","));
console.log(
  "empty:",
  [...db.prepare("SELECT * FROM items WHERE id = -1").iterate()].length,
);
db.close();
