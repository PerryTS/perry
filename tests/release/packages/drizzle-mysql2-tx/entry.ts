// #9356 acceptance: drizzle-orm transactions over the native `mysql2`
// binding (perry-ext-mysql2), a few hundred in a row, with the young
// generation collected every couple of transactions (fixture.sh sets
// PERRY_GC_SCAVENGE_NURSERY_MB=1).
//
// Before the fix the promise minted by `perry_ffi_promise_new` lived in the
// nursery; the first copying minor that landed while a query was in flight
// moved it under the worker thread's raw pointer, the worker then settled
// the retired copy, and the transaction never completed (~195 transactions
// with a 16 MB nursery, iteration 2 with a 1 MB one). Every query also
// leaked its native-async token, and the top-level `await` parked for the
// full 1 s idle budget after the drain had already settled its promise.
import mysql from "mysql2/promise";
import { drizzle } from "drizzle-orm/mysql2";
import { sql } from "drizzle-orm";

const pool = mysql.createPool({
    host: "127.0.0.1",
    port: 3306,
    user: process.env.PERRY_FIXTURE_MYSQL_USER ?? "root",
    password: process.env.PERRY_FIXTURE_MYSQL_PASSWORD ?? "",
    database: "perry_drizzle_test",
});
// `any`: the statically typed `db.transaction(...)` call currently throws
// "Cannot convert undefined or null to object" on main (typed-dispatch
// regression, tracked separately); the reporter's code hits the same
// machinery through the dynamic path this exercises.
const db: any = drizzle(pool, { mode: "default" });

await db.execute(sql`DROP TABLE IF EXISTS tx_notifications`);
await db.execute(sql`CREATE TABLE tx_notifications (id INT PRIMARY KEY AUTO_INCREMENT, body VARCHAR(32) NOT NULL)`);
for (let i = 1; i <= 9; i++) {
    await db.execute(sql`INSERT INTO tx_notifications (body) VALUES (${"n" + i})`);
}
console.log("seeded");

const ITERATIONS = 400;
let completed = 0;
for (let iteration = 1; iteration <= ITERATIONS; iteration++) {
    await db.transaction(async (tx: any) => {
        const result = await tx.execute(sql`
            SELECT a.id
              FROM tx_notifications a
              CROSS JOIN tx_notifications b
             ORDER BY a.id, b.id
        `);
        const rows = (Array.isArray(result) ? result[0] : result) as Array<{ id: number }>;
        if (rows.length !== 81) {
            throw new Error(`iteration ${iteration}: expected 81 rows, received ${rows.length}`);
        }
    });
    completed++;
}
console.log(`transactions=${completed}/${ITERATIONS}`);

await pool.end();
console.log("done");
