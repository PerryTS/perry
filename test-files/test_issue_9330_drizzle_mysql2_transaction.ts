// parity-skip: requires drizzle-orm, mysql2, and a live MySQL fixture
// End-to-end regression for #9330. Compile with Cargo absent from PATH to
// exercise the bundled sqlx-backed mysql2 implementation.
// platforms: skip

import mysql from 'mysql2/promise';
import { drizzle } from 'drizzle-orm/mysql2';
import { sql } from 'drizzle-orm';

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function connectionId(result: any): number {
  const rows = Array.isArray(result) ? result[0] : result;
  return Number(rows[0].id);
}

const pool: any = mysql.createPool({
  host: process.env.DB_HOST ?? '127.0.0.1',
  port: Number(process.env.DB_PORT ?? 3306),
  user: process.env.DB_USER,
  password: process.env.DB_PASSWORD,
  database: process.env.DB_NAME,
  // These mysql2-only keys select Perry's native implementation.
  connectionLimit: 4,
  waitForConnections: true,
});
const db: any = drizzle(pool, { mode: 'default' });

assert('getConnection' in pool, 'Drizzle must recognize the mysql2 pool');
const ids: number[] = [];
await db.transaction(async (tx: any) => {
  ids.push(connectionId(await tx.execute(sql`SELECT CONNECTION_ID() AS id`)));
  ids.push(connectionId(await tx.execute(sql`SELECT CONNECTION_ID() AS id`)));
  ids.push(connectionId(await tx.execute(sql`SELECT CONNECTION_ID() AS id`)));
  // Acquire a real row lock without changing fixture data.
  await tx.execute(sql`UPDATE notifications SET body = body ORDER BY id LIMIT 1`);
});

assert(ids[0] > 0, 'transaction did not return a connection id');
assert(ids.every((id) => id === ids[0]), `transaction scattered across: ${ids.join(',')}`);
if (process.env.DB_CHECK_INNODB_TRX === '1') {
  // Requires MySQL's PROCESS privilege; connection-id stability above is the
  // portable atomicity assertion, while this additionally catches lock leaks.
  const [openTransactions] = await pool.query(
    'SELECT trx_id FROM information_schema.innodb_trx WHERE trx_mysql_thread_id = ?',
    [ids[0]],
  );
  assert(openTransactions.length === 0, 'transaction connection remained open after commit');
}

await pool.end();
console.log('issue 9330 drizzle mysql2 transaction: OK');
