// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", String(fn()));
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:");
const statement = db.prepare("SELECT 1 AS value");
const session = db.createSession();
console.log(
  "dispose shapes:",
  typeof (db as any)[Symbol.dispose],
  typeof (session as any)[Symbol.dispose],
);
probe("session dispose", () => (session as any)[Symbol.dispose]());
probe("session dispose twice", () => (session as any)[Symbol.dispose]());
probe("session changeset", () => session.changeset());

probe("database dispose", () => (db as any)[Symbol.dispose]());
console.log("database closed:", db.isOpen);
probe("database dispose twice", () => (db as any)[Symbol.dispose]());
probe("statement finalized", () => statement.get());

const alreadyClosed = new DatabaseSync(":memory:");
alreadyClosed.close();
probe("dispose closed", () => (alreadyClosed as any)[Symbol.dispose]());
