// Refs #11919 item 7 — better-sqlite3 close() must eagerly release SQLite.
import Database from "better-sqlite3";
import { readFileSync, readdirSync, unlinkSync } from "node:fs";

declare function gc(): void;

const path = `/tmp/perry-better-close-${process.pid}.db`;
const closedMessage = "The database connection is not open";
let diagnosticFailures = 0;

function removeDatabase(): void {
  try {
    unlinkSync(path);
  } catch {}
}

function caught(label: string, operation: () => unknown): void {
  try {
    operation();
    console.log(label, "NO_THROW");
    diagnosticFailures++;
  } catch (error) {
    const caughtError = error as Error;
    console.log(label, caughtError.name, caughtError.message);
    if (caughtError.name !== "TypeError" || caughtError.message !== closedMessage) {
      diagnosticFailures++;
    }
  }
}

function rssKb(): number {
  const status = readFileSync("/proc/self/status", "utf8");
  const line = status.split("\n").find((entry) => entry.startsWith("VmRSS:"));
  if (!line) throw new Error("VmRSS is missing from /proc/self/status");
  return Number(line.trim().split(/\s+/)[1]);
}

function fdCount(): number {
  return readdirSync("/proc/self/fd").length;
}

function collect(): void {
  if (typeof gc === "function") gc();
}

removeDatabase();

const db = new Database(path);
db.exec("CREATE TABLE items (value INTEGER)");
const select = db.prepare("SELECT value FROM items");
const insert = db.prepare("INSERT INTO items VALUES (1)");
db.close();

// close() is idempotent, but every operation that needs the connection must
// report better-sqlite3's exact plain-TypeError diagnostic.
db.close();
console.log("second close ok");
caught("prepare", () => db.prepare("SELECT 1"));
caught("exec", () => db.exec("SELECT 1"));
caught("stmt get", () => select.get());
caught("stmt all", () => select.all());
caught("stmt run", () => insert.run());

// Warm the allocator and handle-registration path before measuring. With the
// old no-op close(), every remaining iteration retained a Connection and an
// open descriptor; with eager drop, both measurements plateau.
for (let i = 0; i < 250; i++) {
  const warm = new Database(path);
  warm.close();
}
collect();

const fdsBefore = fdCount();
let rssPlateauStart = 0;
for (let i = 0; i < 10_000; i++) {
  const current = new Database(path);
  current.close();
  if (i % 100 === 99) collect();
  if (i === 4_999) rssPlateauStart = rssKb();
}
collect();
const fdsAfter = fdCount();
const rssAfter = rssKb();

const fdGrowth = fdsAfter - fdsBefore;
const rssGrowthKb = rssAfter - rssPlateauStart;
if (fdGrowth > 2) {
  throw new Error(`file descriptor leak: grew by ${fdGrowth}`);
}
if (rssGrowthKb > 8 * 1024) {
  throw new Error(`RSS leak: grew by ${rssGrowthKb} KiB`);
}
console.log("10k close resources flat", fdGrowth <= 2, rssGrowthKb <= 8 * 1024);

removeDatabase();

if (diagnosticFailures !== 0) {
  throw new Error(`${diagnosticFailures} closed-connection diagnostics were wrong`);
}
