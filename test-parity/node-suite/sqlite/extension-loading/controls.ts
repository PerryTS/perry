// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const missingExtensionPath = "/tmp/perry-node-sqlite-missing-extension";

function summarize(value) {
  return value === undefined ? "undefined" : String(value);
}

function summarizeError(error) {
  return `${error.name}:${error.code || "nocode"}`;
}

function report(label, fn) {
  try {
    console.log(label, "OK", summarize(fn()));
  } catch (error) {
    console.log(label, "THROW", summarizeError(error));
  }
}

console.log("constructor:", typeof DatabaseSync);

const defaultDb = new DatabaseSync(":memory:");
console.log("method shapes:", typeof defaultDb.enableLoadExtension, typeof defaultDb.loadExtension);

report("default enable false", () => defaultDb.enableLoadExtension(false));
report("default enable true", () => defaultDb.enableLoadExtension(true));
report("default load", () => defaultDb.loadExtension(missingExtensionPath));

for (const value of [undefined, null, 0, "true"]) {
  report(`enable arg ${String(value)}`, () => defaultDb.enableLoadExtension(value));
}

for (const value of [null, 1, "true"]) {
  report(`constructor allow ${String(value)}`, () => {
    const db = new DatabaseSync(":memory:", { allowExtension: value });
    db.close();
    return "created";
  });
}

const enabledDb = new DatabaseSync(":memory:", { allowExtension: true });
report("enabled enable true", () => enabledDb.enableLoadExtension(true));
report("enabled load missing", () => enabledDb.loadExtension(missingExtensionPath));
report("enabled disable", () => enabledDb.enableLoadExtension(false));
report("enabled load disabled", () => enabledDb.loadExtension(missingExtensionPath));
