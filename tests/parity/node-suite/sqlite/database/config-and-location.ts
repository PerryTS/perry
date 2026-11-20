// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", String(fn()));
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

function defensiveMode(db: DatabaseSync) {
  db.exec("PRAGMA journal_mode=OFF");
  return (db.prepare("PRAGMA journal_mode").get() as any).journal_mode;
}

const defaults = new DatabaseSync(":memory:");
console.log("default defensive:", defensiveMode(defaults));
console.log(
  "memory location:",
  String(defaults.location()),
  String(defaults.location("main")),
);
probe("location type", () => defaults.location(1 as any));
probe("location unknown", () => defaults.location("missing"));
defaults.close();

const disabled = new DatabaseSync(":memory:", { defensive: false });
console.log("disabled defensive:", defensiveMode(disabled));
disabled.close();

const enabled = new DatabaseSync(":memory:", { defensive: false });
enabled.enableDefensive(true);
console.log("enabled defensive:", defensiveMode(enabled));
probe("defensive value", () => enabled.enableDefensive(1 as any));
enabled.close();

const toggledOff = new DatabaseSync(":memory:");
toggledOff.enableDefensive(false);
console.log("toggled off defensive:", defensiveMode(toggledOff));
toggledOff.close();

for (const value of [null, 0, "false", {}]) {
  probe(`defensive option ${value === null ? "null" : typeof value}`, () => {
    const db = new DatabaseSync(":memory:", { defensive: value as any });
    db.close();
  });
}

const dqs = new DatabaseSync(":memory:", {
  enableDoubleQuotedStringLiterals: true,
});
console.log(
  "dqs enabled:",
  (dqs.prepare('SELECT "literal" AS value').get() as any).value,
);
dqs.close();

const noDqs = new DatabaseSync(":memory:");
probe("dqs default", () => noDqs.prepare('SELECT "literal" AS value').get());
noDqs.close();
