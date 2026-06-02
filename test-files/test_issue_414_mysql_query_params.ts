// Regression test for issue #414:
// `db.query(sql, [param])` against MySQL failed with `1835 (HY000):
// Malformed communication packet` and left the connection unusable.
//
// Root cause: the codegen dispatch table for ("mysql2", "Pool", "query")
// emits 3 args (handle + sql + params), but the runtime function
// `js_mysql2_pool_query` only declared 2 args — silently dropping the
// params on the floor. sqlx then sent the binary-protocol execute frame
// with 1 placeholder but 0 bind values; MySQL rejected the malformed
// packet and the connection stuck.
//
// Fix: thread `params: JSValue` through the three `*_query` runtime
// functions and bind via the same `ParamValue` / sqlx::query.bind path
// used by the `*_execute` siblings.
//
// This file documents the user repro shape. The runtime fix is unit-
// tested in crates/perry-stdlib/src/mysql2/pool.rs::tests (no live
// MySQL server needed). End-to-end verification requires a MySQL server
// at localhost:3306 with user "perry" / password "perry" / database
// "perry_hub" (mirroring the issue's repro env). When run against such a
// server, all four labelled cases must succeed:
//
//   [1] no-param query (already worked pre-fix)
//   [2] parameterized query — single int bound param
//   [3] parameterized query — single string bound param
//   [4] db.execute with prepared params (mysql2 binary protocol)
//
// platforms: skip
// Skipped by default: CI runners do not have a MySQL server. To run
// locally: spin up MySQL via docker, drop the `// platforms: skip`
// banner, and rebuild.

import mysql from 'mysql2/promise';

const db = mysql.createPool({
  host: 'localhost', port: 3306, user: 'perry', password: 'perry',
  database: 'perry_hub',
});

async function main(): Promise<void> {
  console.log('[1] no-param query...');
  const a: any = await db.query('SELECT 1 AS one');
  console.log('[1] OK rows=' + JSON.stringify(a[0]));

  console.log('[2] parameterized query (literal num)...');
  try {
    const b: any = await db.query('SELECT ? AS x', [42]);
    console.log('[2] OK rows=' + JSON.stringify(b[0]));
  } catch (e: any) {
    console.log('[2] FAILED: ' + (e.message || e));
  }

  console.log('[3] parameterized query (literal string)...');
  try {
    const c: any = await db.query('SELECT ? AS x', ['hello']);
    console.log('[3] OK rows=' + JSON.stringify(c[0]));
  } catch (e: any) {
    console.log('[3] FAILED: ' + (e.message || e));
  }

  console.log('[4] db.execute with params...');
  try {
    const d: any = await db.execute('SELECT ? AS x', ['hello']);
    console.log('[4] OK rows=' + JSON.stringify(d[0]));
  } catch (e: any) {
    console.log('[4] FAILED: ' + (e.message || e));
  }

  process.exit(0);
}

main();
setTimeout(() => { console.log('[T] timeout'); process.exit(2); }, 10000);
