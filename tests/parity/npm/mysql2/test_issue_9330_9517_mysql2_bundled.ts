// parity-skip: requires a live MySQL fixture and the bundled mysql2 fallback
// Regression coverage for #9330 and #9517. Run with DB_HOST, DB_PORT,
// DB_USER, DB_PASSWORD, and DB_NAME set, and compile without Cargo on PATH so
// Perry selects its bundled mysql2 implementation.
// platforms: skip

import mysql from 'mysql2/promise';

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function main(): Promise<void> {
  const pool: any = mysql.createPool({
    host: process.env.DB_HOST ?? '127.0.0.1',
    port: Number(process.env.DB_PORT ?? 3306),
    user: process.env.DB_USER,
    password: process.env.DB_PASSWORD,
    database: process.env.DB_NAME,
  });

  assert('getConnection' in pool, 'getConnection must be visible to `in`');
  assert(Reflect.has(pool, 'getConnection'), 'getConnection must be reflectable');
  assert(typeof pool.getConnection === 'function', 'getConnection must be callable');

  // A dynamically typed receiver must still route to mysql2 instead of being
  // interpreted as a raw object pointer.
  const dynamic: any = pool;
  const [dynamicRows] = await dynamic.query('SELECT 9517 AS issue');
  assert(dynamicRows[0].issue === 9517, 'dynamic query returned the wrong row');

  // The options-object form is used by Drizzle. `values` is accepted from the
  // object and rowsAsArray changes the row representation.
  const [arrayRows] = await pool.query({
    sql: 'SELECT ? AS issue',
    values: [9330],
    rowsAsArray: true,
  });
  assert(Array.isArray(arrayRows[0]), 'rowsAsArray did not return positional rows');
  assert(arrayRows[0][0] === 9330, 'options.values was not bound');

  // mysql2 query() without values uses the text protocol. MySQL rejects BEGIN
  // when it is sent through COM_STMT_PREPARE.
  await pool.query('BEGIN');
  await pool.query('ROLLBACK');

  let syntaxError: any;
  try {
    await pool.query('SELEC broken syntax');
  } catch (error: any) {
    syntaxError = error;
  }
  assert(syntaxError instanceof Error, 'query rejection must be an Error');
  assert(typeof syntaxError.message === 'string', 'query Error needs .message');
  assert(syntaxError.code === 'ER_PARSE_ERROR', 'query Error needs mysql2 .code');
  assert(syntaxError.errno === 1064, 'query Error needs mysql2 .errno');

  const getConnection = pool.getConnection;
  if (process.env.DB_USE_EXISTING_NOTIFICATIONS === '1') {
    // Useful on constrained test hosts where the fixture table already exists
    // but creating another InnoDB tablespace is not possible.
    const [beforeRows] = await pool.query(
      'SELECT id, body FROM notifications ORDER BY id LIMIT 1',
    );
    assert(beforeRows.length === 1, 'notifications fixture needs one row');
    const before = beforeRows[0];
    const connection: any = await getConnection();
    try {
      await connection.beginTransaction();
      await connection.execute('UPDATE notifications SET body = ? WHERE id = ?', [
        'tx-9330',
        before.id,
      ]);
      await connection.rollback();
    } finally {
      connection.release();
    }
    const [afterRows] = await pool.query('SELECT body FROM notifications WHERE id = ?', [
      before.id,
    ]);
    assert(afterRows[0].body === before.body, 'rollback escaped its checked-out connection');
  } else {
    await pool.query('DROP TABLE IF EXISTS perry_issue_9330');
    await pool.query('CREATE TABLE perry_issue_9330 (value INT NOT NULL)');
    const connection: any = await getConnection();
    try {
      await connection.beginTransaction();
      await connection.execute('INSERT INTO perry_issue_9330 (value) VALUES (?)', [1]);
      await connection.rollback();
    } finally {
      connection.release();
    }
    const [countRows] = await pool.query('SELECT COUNT(*) AS count FROM perry_issue_9330');
    assert(countRows[0].count === 0, 'rollback escaped its checked-out connection');
  }

  await pool.end();
  console.log('issues 9330/9517 mysql2 bundled: OK');
}

main();
