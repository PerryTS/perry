// A better-sqlite3 Database (and the Statements it prepares) reached through
// an untyped value: an any local, an object field, a Map value. The handle
// ids were claimed by no dynamic dispatch, so every such call silently
// returned undefined; only statically typed calls worked.
import Database from "better-sqlite3";

const db = new Database(":memory:");
const anyDb: any = db;
anyDb.exec("create table t (a integer, b text)");
const ins: any = anyDb.prepare("insert into t values (?, ?)");
console.log("run", JSON.stringify(ins.run(1, "x")));
ins.run(2, "y");
console.log("any", JSON.stringify(anyDb.prepare("select a, b from t order by a").all()));

const holder: any = { native: db };
console.log("get", JSON.stringify(holder.native.prepare("select b from t where a = ?").get(2)));
console.log("raw", JSON.stringify(holder.native.prepare("select a, b from t order by a").raw().all()));

const services = new Map<string, unknown>([["native", db]]);
const fromMap = services.get("native") as any;
console.log("map", JSON.stringify(fromMap.prepare("select count(*) as n from t").get()));
console.log("typed", JSON.stringify(db.prepare("select a from t order by a").all()));
console.log("chain", typeof anyDb.exec("select 1"));
anyDb.close();
console.log("closed");
