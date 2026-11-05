import { DatabaseSync } from "node:sqlite";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (e) {
    return (e as any)?.code || (e as Error)?.name || String(e);
  }
}

const missingExtension = "/definitely-not-a-perry-sqlite-extension-3292";

const db = new DatabaseSync(":memory:");
console.log("enableLoadExtension typeof:", typeof db.enableLoadExtension);
console.log("loadExtension typeof:", typeof db.loadExtension);
console.log("default enable false:", String(db.enableLoadExtension(false)));
console.log("default enable true:", codeOf(() => db.enableLoadExtension(true)));
console.log("default load:", codeOf(() => db.loadExtension(missingExtension)));
console.log(
  "bad allowExtension:",
  codeOf(() => new DatabaseSync(":memory:", { allowExtension: 1 as any })),
);
console.log(
  "bad enable missing:",
  codeOf(() => (db as any).enableLoadExtension()),
);
console.log(
  "bad enable number:",
  codeOf(() => db.enableLoadExtension(1 as any)),
);

const allowed = new DatabaseSync(":memory:", { allowExtension: true });
console.log("allowed enable true:", String(allowed.enableLoadExtension(true)));
console.log(
  "allowed load missing:",
  codeOf(() => allowed.loadExtension(missingExtension)),
);
console.log("allowed disable:", String(allowed.enableLoadExtension(false)));
console.log(
  "disabled load:",
  codeOf(() => allowed.loadExtension(missingExtension)),
);
