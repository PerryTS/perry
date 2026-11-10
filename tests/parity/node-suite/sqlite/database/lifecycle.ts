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

console.log("constructor:", typeof DatabaseSync, DatabaseSync.length);
probe("without new", () => (DatabaseSync as any)(":memory:"));

const deferred = new DatabaseSync(":memory:", { open: false });
console.log("deferred initial:", deferred.isOpen);
probe("transaction closed", () => deferred.isTransaction);
probe("exec closed", () => deferred.exec("SELECT 1"));
probe("prepare closed", () => deferred.prepare("SELECT 1"));
probe("close closed", () => deferred.close());
probe("open", () => deferred.open());
console.log("opened:", deferred.isOpen, deferred.isTransaction);
probe("open twice", () => deferred.open());
probe("close", () => deferred.close());
console.log("closed:", deferred.isOpen);
probe("close twice", () => deferred.close());

const immediate = new DatabaseSync(":memory:");
console.log("immediate:", immediate.isOpen, immediate.isTransaction);
immediate.close();
