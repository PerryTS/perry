// #489 acceptance: 50-line drizzle program against a real MySQL via
// `@perryts/mysql` (pure-TS wire-protocol driver compiled natively
// through perry's compilePackages). Uses `drizzle-orm/mysql-proxy`
// (drizzle's callback-based entry) so we never pull in npm `mysql2`.

import { drizzle } from "drizzle-orm/mysql-proxy";
import { mysqlTable, int, varchar } from "drizzle-orm/mysql-core";
import { eq } from "drizzle-orm";
import { connect } from "@perryts/mysql";

const conn = await connect({
    host: "127.0.0.1",
    port: 3306,
    user: "root",
    password: "",
    database: "perry_drizzle_test",
});

// Adapter: @perryts/mysql → drizzle/mysql-proxy callback. `method` is
// "all" for SELECT (returns object rows that drizzle maps via field
// order — convert to row-arrays here) and "execute" for everything
// else (returns `[{ insertId, affectedRows }]` per drizzle's contract).
async function exec(sql: string, params: any[], method: string): Promise<{ rows: any[] }> {
    const result = await conn.query(sql, params);
    if (method === "execute" && result.fields.length === 0) {
        return { rows: [{
            insertId: typeof result.lastInsertId === "bigint" ? Number(result.lastInsertId) : result.lastInsertId,
            affectedRows: result.rowCount,
        }] };
    }
    const fieldNames = result.fields.map((f: any) => f.name);
    const rows = result.rows.map((r: any) => fieldNames.map((n: string) => r[n]));
    return { rows };
}

const db = drizzle(exec);

// Idiomatic drizzle: `await db.select().from(users)` resolves via the
// QueryPromise.then thenable that `applyMixins(MySqlSelectBase,
// [QueryPromise])` copies onto MySqlSelectBase.prototype. Closed by #2159.


const users = mysqlTable("users", {
    id: int("id").primaryKey().autoincrement(),
    name: varchar("name", { length: 64 }).notNull(),
    age: int("age").notNull(),
});
const posts = mysqlTable("posts", {
    id: int("id").primaryKey().autoincrement(),
    userId: int("user_id").notNull(),
    title: varchar("title", { length: 128 }).notNull(),
});

// Fresh schema each run so we're deterministic.
await exec("DROP TABLE IF EXISTS posts", [], "execute");
await exec("DROP TABLE IF EXISTS users", [], "execute");
await exec("CREATE TABLE users (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(64) NOT NULL, age INT NOT NULL)", [], "execute");
await exec("CREATE TABLE posts (id INT PRIMARY KEY AUTO_INCREMENT, user_id INT NOT NULL, title VARCHAR(128) NOT NULL)", [], "execute");

await db.insert(users).values([
    { id: 1, name: "alice", age: 30 },
    { id: 2, name: "bob",   age: 25 },
]);
await db.insert(posts).values([
    { id: 1, userId: 1, title: "hello" },
    { id: 2, userId: 1, title: "world" },
]);

const all = await db.select().from(users);
console.log(`count=${all.length}`);

await db.update(users).set({ age: 31 }).where(eq(users.id, 1));
const a = await db.select().from(users).where(eq(users.id, 1));
console.log(`alice.age=${a[0].age}`);

await db.delete(users).where(eq(users.id, 2));
console.log(`after_delete=${(await db.select().from(users)).length}`);

const joined = await db.select().from(users).leftJoin(posts, eq(users.id, posts.userId));
console.log(`join_rows=${joined.length}`);

await conn.close();
