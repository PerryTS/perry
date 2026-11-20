// parity-node-argv: --experimental-sqlite
// node:sqlite DatabaseSync lifecycle conformance (#6561): constructor
// validation, open/close state machine, isOpen/isTransaction, location().
import { DatabaseSync } from "node:sqlite";

function summarizeError(error) {
  return `${error.name}:${error.code || "nocode"}:${String(error.message).split("\n")[0]}`;
}

function report(label, fn) {
  try {
    const value = fn();
    console.log(label, "OK", value === undefined ? "undefined" : String(value));
  } catch (error) {
    console.log(label, "THROW", summarizeError(error));
  }
}

const db = new DatabaseSync(":memory:");
console.log("fresh isOpen:", db.isOpen, "isTransaction:", db.isTransaction);
console.log("location:", db.location());

report("open while open", () => db.open());
db.close();
console.log("closed isOpen:", db.isOpen);
report("close while closed", () => db.close());
report("exec while closed", () => db.exec("SELECT 1"));
report("prepare while closed", () => db.prepare("SELECT 1"));

report("deferred open", () => {
  const deferred = new DatabaseSync(":memory:", { open: false });
  const before = deferred.isOpen;
  deferred.open();
  const after = deferred.isOpen;
  deferred.close();
  return `${before}->${after}`;
});

report("path number", () => new DatabaseSync(42 as any));
report("path null", () => new DatabaseSync(null as any));
report("path undefined", () => new DatabaseSync(undefined as any));
report("path uint8array", () => {
  const bytes = new TextEncoder().encode(":memory:");
  const u8db = new DatabaseSync(bytes as any);
  const open = u8db.isOpen;
  u8db.close();
  return open;
});

report("options null", () => new DatabaseSync(":memory:", null as any));
report("transaction state", () => {
  const t = new DatabaseSync(":memory:");
  t.exec("CREATE TABLE x(a)");
  t.exec("BEGIN");
  const during = t.isTransaction;
  t.exec("ROLLBACK");
  const after = t.isTransaction;
  t.close();
  return `${during}->${after}`;
});
