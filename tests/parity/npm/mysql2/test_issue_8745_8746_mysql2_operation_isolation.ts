// parity-skip: requires a live MySQL fixture; native wrapper unit-tested
// Regression coverage for issues #8745 and #8746.
//
// Run against a local MySQL database after removing the skip marker. The loop
// mixes text-protocol DDL with prepared statements of different arities; every
// statement must retain its own parameter vector. The checked-out connection
// then exercises the canonical row-lock transaction lifecycle.
//
// platforms: skip

import mysql from 'mysql2/promise';

const pool = mysql.createPool({
  host: 'localhost', port: 3306, user: 'perry', password: 'perry',
  database: 'perry_hub',
});

async function main(): Promise<void> {
  for (let round = 0; round < 25; round++) {
    await pool.query('DROP TABLE IF EXISTS perry_issue_8745');
    await pool.query(
      'CREATE TABLE perry_issue_8745 (' +
      'id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50), cents INT)',
    );
    await pool.execute(
      'INSERT INTO perry_issue_8745 (name, cents) VALUES (?, ?)',
      ['round-' + round, 100 + round],
    );
    const selected: any = await pool.execute(
      'SELECT * FROM perry_issue_8745 WHERE id = ?',
      [1],
    );
    if (selected[0][0].cents !== 100 + round) {
      throw new Error('wrong prepared-statement parameters in round ' + round);
    }
  }

  const connection = await pool.getConnection();
  try {
    await connection.beginTransaction();
    const locked: any = await connection.execute(
      'SELECT cents FROM perry_issue_8745 WHERE id = ? FOR UPDATE',
      [1],
    );
    await connection.execute(
      'UPDATE perry_issue_8745 SET cents = ? WHERE id = ?',
      [locked[0][0].cents + 1, 1],
    );
    await connection.commit();

    await connection.beginTransaction();
    await connection.execute(
      'UPDATE perry_issue_8745 SET cents = ? WHERE id = ?',
      [9999, 1],
    );
    await connection.rollback();
  } finally {
    connection.release();
  }

  const finalRows: any = await pool.execute(
    'SELECT cents FROM perry_issue_8745 WHERE id = ?',
    [1],
  );
  console.log(finalRows[0][0].cents);
  await pool.end();
}

main();
