// #9935: check Drizzle's SQL/parameter prefixes before they reach a driver.
// This is an investigation probe, not a demonstrated reproduction of the
// production failure. Keep both the original four predicates and a wider
// variant that forces the SQL chunk/parameter arrays to grow repeatedly.
import { and, asc, eq, isNotNull, lte } from "drizzle-orm";
import { datetime, int, mysqlTable, QueryBuilder, varchar } from "drizzle-orm/mysql-core";

declare function gc(): void;

const iterations = Number(process.env.PERRY_SQL_PREFIX_ITERATIONS ?? "1000");
if (!Number.isInteger(iterations) || iterations < 1 || iterations > 1000000) {
    throw new Error("PERRY_SQL_PREFIX_ITERATIONS must be an integer from 1 to 1000000");
}
if (typeof gc !== "function") {
    throw new Error("explicit GC is required; use node --expose-gc for the oracle");
}

const auctions = mysqlTable("auctions", {
    id: int("id").primaryKey(),
    status: varchar("status", { length: 32 }),
    format: varchar("format", { length: 32 }),
    endsAt: datetime("endsAt"),
});
const builder = new QueryBuilder();
const at = new Date("2026-09-07T12:00:00.000Z");
const expectedDate = "2026-09-07 12:00:00.000";
const prefix = "select `id` from `auctions` where (";
const baseConditions = "`auctions`.`status` = ? and `auctions`.`format` = ? and `auctions`.`endsAt` is not null and `auctions`.`endsAt` <= ?";
const suffix = ") order by `auctions`.`endsAt` asc limit ?";
type Query = { sql: string; params: unknown[] };

function check(query: Query, expectedSql: string, expectedParams: unknown[], context: string) {
    if (query.sql !== expectedSql) {
        throw new Error(context + ": SQL mismatch\nexpected: " + expectedSql + "\nactual: " + query.sql);
    }
    if (!Array.isArray(query.params) || query.params.length !== expectedParams.length) {
        throw new Error(context + ": parameter length mismatch, expected " + expectedParams.length);
    }
    for (let index = 0; index < expectedParams.length; index++) {
        if (query.params[index] !== expectedParams[index]) {
            throw new Error(context + ": parameter " + index + " mismatch, expected " + expectedParams[index] + ", actual " + query.params[index]);
        }
    }
}

// Fixed expected structure; changing iteration values prevent a cached answer
// or a result from an earlier query from satisfying the assertions.
const width = 40;
let wideConditions = baseConditions;
for (let index = 0; index < width; index++) {
    wideConditions += " and `auctions`.`id` = ?";
}
let checked = 0;
let collections = 0;
let previous: Query | undefined;
let previousParams: unknown[] = [];
for (let iteration = 0; iteration < iterations; iteration++) {
    const status = "live-" + iteration;
    const format = "auction-" + iteration;
    const limit = 50 + iteration % 7;
    const originalParams: unknown[] = [status, format, expectedDate, limit];
    const original = builder.select({ id: auctions.id }).from(auctions).where(and(
        eq(auctions.status, status),
        eq(auctions.format, format),
        isNotNull(auctions.endsAt),
        lte(auctions.endsAt, at),
    ));
    if (iteration % 16 === 0) {
        // Keep the already-built head alive while the collector runs, before
        // the orderBy/limit tail is appended (the reported missing-head shape).
        gc();
        collections++;
    }
    const query = original.orderBy(asc(auctions.endsAt)).limit(limit).toSQL();
    check(query, prefix + baseConditions + suffix, originalParams, "original iteration " + iteration);
    checked++;

    const conditions = [
        eq(auctions.status, status), eq(auctions.format, format),
        isNotNull(auctions.endsAt), lte(auctions.endsAt, at),
    ];
    const wideParams: unknown[] = [status, format, expectedDate];
    for (let index = 0; index < width; index++) {
        const value = iteration * width + index;
        conditions.push(eq(auctions.id, value));
        wideParams.push(value);
    }
    wideParams.push(limit);
    const wide = builder.select({ id: auctions.id }).from(auctions)
        .where(and(...conditions)).orderBy(asc(auctions.endsAt)).limit(limit).toSQL();
    if (iteration % 16 === 0) {
        gc();
        collections++;
    }
    check(wide, prefix + wideConditions + suffix, wideParams, "wide iteration " + iteration);
    check(query, prefix + baseConditions + suffix, originalParams, "retained current " + iteration);
    checked += 2;
    if (previous !== undefined) {
        check(previous, prefix + wideConditions + suffix, previousParams, "retained previous " + iteration);
        checked++;
    }
    previous = wide;
    previousParams = wideParams;
}
if (checked !== 4 * iterations - 1 || collections === 0) {
    throw new Error("the stress probe did not exercise every assertion and collection window");
}
console.log("sql-prefix-stress: iterations=" + iterations + " checked=" + checked + " explicit_gc=" + collections);
