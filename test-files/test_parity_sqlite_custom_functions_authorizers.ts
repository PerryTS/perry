import * as sqlite from "node:sqlite";
import { DatabaseSync } from "node:sqlite";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (e) {
    return (e as any)?.code || (e as Error)?.name || String(e);
  }
}

function messageOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (e) {
    return (e as Error).message;
  }
}

function scalar(db: any, sql: string): any {
  return db.prepare(sql).get().n;
}

const { constants } = sqlite as any;

console.log(
  "method shape:",
  typeof DatabaseSync,
  typeof DatabaseSync.prototype.function,
  typeof DatabaseSync.prototype.aggregate,
  typeof DatabaseSync.prototype.enableDefensive,
  typeof DatabaseSync.prototype.setAuthorizer,
);
console.log(
  "authorizer constants:",
  constants.SQLITE_OK,
  constants.SQLITE_DENY,
  constants.SQLITE_IGNORE,
  constants.SQLITE_READ,
  constants.SQLITE_SELECT,
);

const db = new DatabaseSync(":memory:");
console.log(
  "instance method shape:",
  typeof db.function,
  typeof db.aggregate,
  typeof db.enableDefensive,
  typeof db.setAuthorizer,
);

db.function("add2", (a: number, b: number) => a + b);
console.log("scalar add:", scalar(db, "SELECT add2(2, 3) AS n"));
console.log("scalar arity:", codeOf(() => scalar(db, "SELECT add2(2) AS n")));

db.function("count_args", { varargs: true }, (...args: unknown[]) => args.length);
console.log(
  "scalar varargs:",
  scalar(db, "SELECT count_args() AS n"),
  scalar(db, "SELECT count_args(1, 2, 3) AS n"),
);

db.function("big_arg", { useBigIntArguments: true }, (value: bigint) => `${typeof value}:${value}`);
console.log("scalar bigint arg:", scalar(db, "SELECT big_arg(9007199254740992) AS n"));
console.log(
  "scalar unsafe arg:",
  codeOf(() => {
    db.function("unsafe_arg", (value: number) => value);
    scalar(db, "SELECT unsafe_arg(9007199254740992) AS n");
  }),
);

db.function("nullish", () => undefined);
console.log("scalar undefined return:", String(scalar(db, "SELECT nullish() AS n")));
console.log(
  "scalar bad return:",
  codeOf(() => {
    db.function("bad_return", () => true);
    scalar(db, "SELECT bad_return() AS n");
  }),
);
console.log(
  "scalar thrown:",
  codeOf(() => {
    db.function("throws_now", () => {
      throw new Error("sqlite callback failed");
    });
    scalar(db, "SELECT throws_now() AS n");
  }),
);
db.function("blob_len", (bytes: Uint8Array) => `${bytes instanceof Uint8Array}:${bytes.length}:${bytes[0]}`);
console.log("scalar blob arg:", scalar(db, "SELECT blob_len(x'010203') AS n"));
db.function("blob_ret", () => new Uint8Array([4, 5, 6]));
const blob = scalar(db, "SELECT blob_ret() AS n");
console.log("scalar blob return:", blob instanceof Uint8Array, blob.length, blob[2]);

console.log("function bad name:", codeOf(() => db.function(1 as any, () => 1)));
console.log("function bad callback:", codeOf(() => db.function("x", undefined as any)));
console.log(
  "function bad option:",
  codeOf(() => db.function("x", { useBigIntArguments: 1 as any }, () => 1)),
);

db.exec("CREATE TABLE nums (grp INTEGER, value INTEGER)");
db.exec("INSERT INTO nums VALUES (1, 1), (1, 2), (2, 3)");
db.aggregate("sum_times", {
  start: 0,
  step: (state: number, value: number) => state + value,
  result: (state: number) => state * 10,
});
console.log("aggregate result:", JSON.stringify(db.prepare("SELECT sum_times(value) AS n FROM nums GROUP BY grp ORDER BY grp").all()));

let starts = 0;
db.aggregate("start_fn", {
  start: () => {
    starts += 1;
    return 10;
  },
  step: (state: number, value: number) => state + value,
});
console.log("aggregate start fn:", scalar(db, "SELECT start_fn(value) AS n FROM nums"), starts);

const shared = { total: 0 };
db.aggregate("shared_obj", {
  start: shared,
  step: (state: any, value: number) => {
    state.total += value;
    return state;
  },
  result: (state: any) => state.total,
});
console.log("aggregate object start:", JSON.stringify(db.prepare("SELECT shared_obj(value) AS n FROM nums GROUP BY grp ORDER BY grp").all()));

db.aggregate("big_sum", {
  start: 0n,
  useBigIntArguments: true,
  step: (state: bigint, value: bigint) => state + value,
  result: (state: bigint) => `${typeof state}:${state}`,
});
console.log("aggregate bigint:", scalar(db, "SELECT big_sum(value) AS n FROM nums"));

db.aggregate("win_sum", {
  start: 0,
  step: (state: number, value: number) => state + value,
  inverse: (state: number, value: number) => state - value,
  result: (state: number) => state,
});
console.log(
  "aggregate inverse:",
  JSON.stringify(db.prepare("SELECT win_sum(value) OVER (ORDER BY value ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) AS n FROM nums ORDER BY value").all()),
);

console.log("aggregate missing start:", codeOf(() => db.aggregate("missing_start", { step: (s: number) => s } as any)));
console.log("aggregate missing step:", codeOf(() => db.aggregate("missing_step", { start: 0 } as any)));

db.exec("CREATE TABLE auth (id INTEGER, secret TEXT)");
db.exec("INSERT INTO auth VALUES (1, 'hidden')");
const seen: any[] = [];
db.setAuthorizer((actionCode: number, arg1: string, arg2: string, dbName: string, trigger: string | null) => {
  if (actionCode === constants.SQLITE_READ && arg1 === "auth" && arg2 === "secret") {
    seen.push([typeof actionCode, arg1, arg2, dbName, trigger === null]);
  }
  return constants.SQLITE_OK;
});
console.log("authorizer allow:", db.prepare("SELECT secret FROM auth").get().secret);
console.log("authorizer args:", JSON.stringify(seen[0]));

db.setAuthorizer((actionCode: number, arg1: string, arg2: string) => {
  if (actionCode === constants.SQLITE_READ && arg1 === "auth" && arg2 === "secret") {
    return constants.SQLITE_IGNORE;
  }
  return constants.SQLITE_OK;
});
console.log("authorizer ignore:", String(db.prepare("SELECT secret FROM auth").get().secret));

db.setAuthorizer((actionCode: number) => {
  return actionCode === constants.SQLITE_SELECT ? constants.SQLITE_DENY : constants.SQLITE_OK;
});
console.log("authorizer deny:", codeOf(() => db.prepare("SELECT secret FROM auth").get()));

db.setAuthorizer(null);
console.log("authorizer clear:", db.prepare("SELECT secret FROM auth").get().secret);
console.log("authorizer bad callback:", codeOf(() => db.setAuthorizer(1 as any)));
db.setAuthorizer(() => "bad" as any);
console.log("authorizer bad type:", messageOf(() => db.prepare("SELECT 1").get()));
db.setAuthorizer(() => 99);
console.log("authorizer bad code:", messageOf(() => db.prepare("SELECT 1").get()));
db.setAuthorizer(null);

const defensive = new DatabaseSync(":memory:");
defensive.exec("CREATE TABLE d (value INTEGER)");
console.log(
  "defensive default:",
  codeOf(() => defensive.exec("PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql='bad' WHERE name='d'")),
);
defensive.enableDefensive(false);
console.log(
  "defensive off:",
  codeOf(() => defensive.exec("PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql='bad' WHERE name='d'")),
);
defensive.enableDefensive(true);
console.log(
  "defensive on:",
  codeOf(() => defensive.exec("UPDATE sqlite_schema SET sql='bad2' WHERE name='d'")),
);
