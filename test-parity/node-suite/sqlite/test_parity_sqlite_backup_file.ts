import * as sqlite from "node:sqlite";
import { DatabaseSync, backup } from "node:sqlite";
import { unlinkSync } from "node:fs";

function cleanup(path: string): void {
  try {
    unlinkSync(path);
  } catch {
  }
}

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (e) {
    return (e as any)?.code || (e as Error)?.name || String(e);
  }
}

async function rejectionCode(fn: () => Promise<unknown>): Promise<string> {
  try {
    await fn();
    return "none";
  } catch (e) {
    return (e as any)?.code || (e as Error)?.name || String(e);
  }
}

const base = `.perry-sqlite-backup-${Date.now()}-${Math.random().toString(16).slice(2)}`;
const sourcePath = `${base}-source.db`;
const copyPath = `${base}-copy.db`;
const bufferCopyPath = `${base}-buffer-copy.db`;
const badSourceCopyPath = `${base}-bad-source-copy.db`;
const closedCopyPath = `${base}-closed-copy.db`;
const missingDirCopyPath = `${base}-missing-dir/copy.db`;

for (const path of [sourcePath, copyPath, bufferCopyPath, badSourceCopyPath, closedCopyPath]) {
  cleanup(path);
}

console.log("sqlite.backup typeof:", typeof sqlite.backup);
console.log("named backup typeof:", typeof backup);

const db = new DatabaseSync(sourcePath);
db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, label TEXT)");
db.prepare("INSERT INTO t (label) VALUES (?)").run("alpha");
db.prepare("INSERT INTO t (label) VALUES (?)").run("beta");

console.log("source location suffix:", String(db.location()?.endsWith(sourcePath)));

const copiedPages = await sqlite.backup(db, copyPath);
console.log("backup pages positive:", String(typeof copiedPages === "number" && copiedPages > 0));

const copy = new DatabaseSync(copyPath);
const rows = copy.prepare("SELECT id, label FROM t ORDER BY id").all();
console.log("copy rows:", rows.map((row: any) => `${row.id}:${row.label}`).join(","));
copy.close();

const bufferPages = await backup(db, Buffer.from(bufferCopyPath));
console.log("buffer path pages positive:", String(typeof bufferPages === "number" && bufferPages > 0));

const bufferCopy = new DatabaseSync(bufferCopyPath);
const count = bufferCopy.prepare("SELECT count(*) AS count FROM t").get() as any;
console.log("buffer path row count:", count.count);
bufferCopy.close();

console.log("missing sourceDb:", codeOf(() => backup()));
console.log("bad sourceDb:", codeOf(() => backup(1 as any, copyPath)));
console.log("missing path:", codeOf(() => backup(db as any)));
console.log("bad path:", codeOf(() => backup(db, 1 as any)));
console.log("bad options:", codeOf(() => backup(db, copyPath, null as any)));
console.log("bad rate:", codeOf(() => backup(db, copyPath, { rate: 1.5 })));
console.log("bad source option:", codeOf(() => backup(db, copyPath, { source: 1 as any })));
console.log("bad target option:", codeOf(() => backup(db, copyPath, { target: false as any })));
console.log("bad progress option:", codeOf(() => backup(db, copyPath, { progress: 1 as any })));

console.log(
  "invalid destination reject:",
  await rejectionCode(() => sqlite.backup(db, missingDirCopyPath)),
);
console.log(
  "invalid source reject:",
  await rejectionCode(() => backup(db, badSourceCopyPath, { source: "missing" })),
);

db.close();
console.log("closed source:", codeOf(() => backup(db, closedCopyPath)));

for (const path of [sourcePath, copyPath, bufferCopyPath, badSourceCopyPath, closedCopyPath]) {
  cleanup(path);
}
