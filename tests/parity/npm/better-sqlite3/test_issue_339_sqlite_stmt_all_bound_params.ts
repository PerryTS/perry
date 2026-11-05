// Regression test for issue #339:
// `db.prepare('SELECT … WHERE x = ?').all('a')` returned an empty array
// even when matching rows existed. Codegen passed the user's single arg
// as a NaN-boxed f64 to a runtime function that expected a packed
// `*const ArrayHeader`; the runtime's defensive "upper-16 != 0 → no
// params" check then dropped every non-undefined arg silently.
// Same root cause for `.get(...)` and `.run(...)`.
import Database from 'better-sqlite3';

const db: any = new Database(':memory:');
db.exec("CREATE TABLE t (x TEXT, n INTEGER); INSERT INTO t VALUES ('a', 1); INSERT INTO t VALUES ('b', 2); INSERT INTO t VALUES ('a', 3);");

// 1. .all() with no params still works (the pre-fix happy path).
const all_no_params = db.prepare('SELECT x FROM t').all();
console.log('all_no_params.length =', all_no_params.length);

// 2. .all('a') with one string-bound param — the issue's repro.
const all_one_str = db.prepare('SELECT x, n FROM t WHERE x = ?').all('a');
console.log('all_one_str.length =', all_one_str.length);
console.log('all_one_str[0].x =', all_one_str[0].x);
console.log('all_one_str[0].n =', all_one_str[0].n);
console.log('all_one_str[1].n =', all_one_str[1].n);

// 3. .all(num, str) — multi-arg bound params with mixed types.
const all_mixed = db.prepare('SELECT x FROM t WHERE n >= ? AND x = ?').all(1, 'a');
console.log('all_mixed.length =', all_mixed.length);

// 4. .get('a') — same root cause; should now return the first matching row.
const get_one = db.prepare('SELECT x, n FROM t WHERE x = ?').get('a');
console.log('get_one.x =', get_one ? get_one.x : 'undefined');
console.log('get_one.n =', get_one ? get_one.n : 'undefined');

// 5. .get() with no params returns the first row of the table.
const get_no_params = db.prepare('SELECT x FROM t').get();
console.log('get_no_params.x =', get_no_params.x);

// 6. .run with bound params actually mutates rows.
const stmt = db.prepare('UPDATE t SET n = ? WHERE x = ?');
const result = stmt.run(99, 'b');
console.log('run.changes =', result.changes);
const after = db.prepare('SELECT n FROM t WHERE x = ?').get('b');
console.log('after.n =', after.n);

process.exit(0);
