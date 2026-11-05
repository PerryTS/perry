import * as sqlite from "node:sqlite";
import { DatabaseSync, Session } from "node:sqlite";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (e) {
    return (e as any)?.code || (e as Error)?.name || String(e);
  }
}

function scalar(db: any, sql: string): any {
  return db.prepare(sql).get().n;
}

const { constants } = sqlite as any;

console.log("Session export typeof:", typeof Session);
console.log("Session namespace typeof:", typeof (sqlite as any).Session);
console.log("Session export name:", (Session as any)?.name);
console.log("Session call:", codeOf(() => (Session as any)()));
console.log("Session new:", codeOf(() => new (Session as any)()));
console.log(
  "Session prototype names:",
  Object.getOwnPropertyNames((Session as any).prototype).join(","),
);
console.log(
  "Session prototype methods:",
  typeof (Session as any).prototype.changeset,
  typeof (Session as any).prototype.patchset,
  typeof (Session as any).prototype.close,
  typeof (Session as any).prototype[Symbol.dispose],
);

console.log("constants typeof:", typeof constants);
console.log(
  "constants changeset:",
  constants.SQLITE_CHANGESET_OMIT,
  constants.SQLITE_CHANGESET_REPLACE,
  constants.SQLITE_CHANGESET_ABORT,
  constants.SQLITE_CHANGESET_CONFLICT,
);
console.log(
  "constants authorizer:",
  constants.SQLITE_OK,
  constants.SQLITE_DENY,
  constants.SQLITE_IGNORE,
  constants.SQLITE_INSERT,
  constants.SQLITE_UPDATE,
  constants.SQLITE_RECURSIVE,
);
console.log(
  "constants keys:",
  Object.keys(constants).includes("SQLITE_CHANGESET_ABORT"),
  Object.keys(constants).includes("SQLITE_CREATE_TABLE"),
);

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)");

const empty = db.createSession();
const emptyCs = empty.changeset();
console.log("empty changeset:", emptyCs instanceof Uint8Array, emptyCs.length);
empty.close();

const session = db.createSession();
console.log(
  "session methods:",
  typeof session.changeset,
  typeof session.patchset,
  typeof session.close,
  typeof (session as any)[Symbol.dispose],
);

db.exec("INSERT INTO t VALUES (1, 'alice')");
const cs = session.changeset();
const ps = session.patchset();
console.log("changeset bytes:", cs instanceof Uint8Array, cs.length > 0);
console.log("patchset bytes:", ps instanceof Uint8Array, ps.length > 0);

const target = new DatabaseSync(":memory:");
target.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)");
console.log("apply result:", target.applyChangeset(cs));
const row = target.prepare("SELECT id, name FROM t").get();
console.log("target row:", row.id, row.name);

const scopedDb = new DatabaseSync(":memory:");
scopedDb.exec("CREATE TABLE a (id INTEGER PRIMARY KEY); CREATE TABLE b (id INTEGER PRIMARY KEY)");
const scopedSession = scopedDb.createSession({ table: "a" });
scopedDb.exec("INSERT INTO a VALUES (1); INSERT INTO b VALUES (2)");
const scopedCs = scopedSession.changeset();
const scopedTarget = new DatabaseSync(":memory:");
scopedTarget.exec("CREATE TABLE a (id INTEGER PRIMARY KEY); CREATE TABLE b (id INTEGER PRIMARY KEY)");
console.log(
  "scoped apply:",
  scopedTarget.applyChangeset(scopedCs),
  scalar(scopedTarget, "SELECT COUNT(*) AS n FROM a"),
  scalar(scopedTarget, "SELECT COUNT(*) AS n FROM b"),
);

const filterTarget = new DatabaseSync(":memory:");
filterTarget.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)");
console.log(
  "filter skip:",
  filterTarget.applyChangeset(cs, { filter: (_name: string) => false }),
  scalar(filterTarget, "SELECT COUNT(*) AS n FROM t"),
);

const conflictTarget = new DatabaseSync(":memory:");
conflictTarget.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)");
conflictTarget.exec("INSERT INTO t VALUES (1, 'old')");
console.log(
  "conflict abort:",
  conflictTarget.applyChangeset(cs, {
    onConflict: (_type: number) => constants.SQLITE_CHANGESET_ABORT,
  }),
  conflictTarget.prepare("SELECT name FROM t WHERE id = 1").get().name,
);

console.log("create bad options:", codeOf(() => db.createSession(null as any)));
console.log("create bad table:", codeOf(() => db.createSession({ table: 1 as any })));
console.log("create bad db:", codeOf(() => db.createSession({ db: 1 as any })));
console.log("apply bad changeset:", codeOf(() => db.applyChangeset(null as any)));
console.log("apply bad options:", codeOf(() => db.applyChangeset(cs, null as any)));
console.log("apply bad filter:", codeOf(() => db.applyChangeset(cs, { filter: 1 as any })));
console.log("apply bad conflict:", codeOf(() => db.applyChangeset(cs, { onConflict: 1 as any })));

session.close();
console.log("session close duplicate:", codeOf(() => session.close()));
console.log("session dispose closed:", String((session as any)[Symbol.dispose]()));
console.log("session changeset closed:", codeOf(() => session.changeset()));

const dbClosed = new DatabaseSync(":memory:");
dbClosed.exec("CREATE TABLE x (id INTEGER PRIMARY KEY)");
const closedSession = dbClosed.createSession();
dbClosed.close();
console.log("session close db closed:", codeOf(() => closedSession.close()));
console.log("session changeset db closed:", codeOf(() => closedSession.changeset()));
console.log("session dispose db closed:", String((closedSession as any)[Symbol.dispose]()));
