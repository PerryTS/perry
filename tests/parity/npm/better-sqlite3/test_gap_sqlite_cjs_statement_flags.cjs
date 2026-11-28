const Database = require("better-sqlite3");
const db = new Database(":memory:");
db.exec("CREATE TABLE t (v INTEGER)");
const cache = new Map();
function statement(sql) {
    if (!cache.has(sql)) cache.set(sql, db.prepare(sql));
    return cache.get(sql);
}
function flags(stmt) {
    return String(stmt.reader) + "/" + String(stmt.readonly);
}
for (const sql of ["SELECT v FROM t", "INSERT INTO t VALUES (1)", "INSERT INTO t VALUES (2) RETURNING v"]) {
    console.log(flags(statement(sql)));
}
console.log("rows=" + db.prepare("SELECT count(*) AS n FROM t").get().n);
db.close();
