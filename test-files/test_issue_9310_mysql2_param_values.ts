// parity-skip: requires a live MySQL fixture; native wrapper unit-tested
// Regression coverage for issue #9310: mysql2 prepared-statement values must
// survive the JS-value -> Rust -> sqlx binding boundary without substitution.
//
// Run with DB_HOST, DB_PORT, DB_USER, DB_PASSWORD, and DB_NAME set. The SQL
// normalizes Date and Buffer parameters to strings so the assertions inspect
// their exact server-observed contents independently of result-decoder types.
// platforms: skip

import mysql from 'mysql2/promise';

function assertJson(label: string, actual: unknown, expected: unknown): void {
  const got = JSON.stringify(actual);
  const want = JSON.stringify(expected);
  if (got !== want) {
    throw new Error(`${label}: expected ${want}, got ${got}`);
  }
}

async function main(): Promise<void> {
  const config = {
    host: process.env.DB_HOST,
    port: Number(process.env.DB_PORT ?? 3306),
    user: process.env.DB_USER,
    password: process.env.DB_PASSWORD,
    database: process.env.DB_NAME,
  };
  const pool = mysql.createPool(config);

  try {
    for (const count of [3, 8, 12, 17]) {
      const sent: (string | null)[] = [];
      for (let i = 0; i < count; i += 1) {
        sent.push(i % 3 === 1 ? null : `v${i}`);
      }
      const sql = 'SELECT ' + sent.map((_, i) => `? AS c${i}`).join(', ');
      const [rows] = await pool.execute(sql, sent);
      const row = (rows as Record<string, unknown>[])[0];
      const actual = sent.map((_, i) => row[`c${i}`]);
      assertJson(`${count}-parameter values`, actual, sent);
    }

    const sentDate = new Date('2024-02-03T04:05:06.789Z');
    const sentBuffer = Buffer.from([0, 1, 127, 128, 255]);
    const [rows] = await pool.execute(
      [
        'SELECT ? AS shortString',
        ', ? AS longString',
        ', ? AS intValue',
        ', ? AS floatValue',
        ', ? AS boolValue',
        ', ? AS nullValue',
        ", DATE_FORMAT(?, '%Y-%m-%dT%H:%i:%s.%f') AS dateValue",
        ', HEX(?) AS bufferHex',
      ].join(''),
      ['hi', 'long-string', 42, 3.25, true, null, sentDate, sentBuffer],
    );
    const row = (rows as Record<string, unknown>[])[0];
    assertJson(
      'typed parameter values',
      {
        shortString: row.shortString,
        longString: row.longString,
        intValue: row.intValue,
        floatValue: row.floatValue,
        boolValue: row.boolValue,
        nullValue: row.nullValue,
        dateValue: row.dateValue,
        bufferHex: row.bufferHex,
      },
      {
        shortString: 'hi',
        longString: 'long-string',
        intValue: 42,
        floatValue: 3.25,
        boolValue: 1,
        nullValue: null,
        dateValue: '2024-02-03T04:05:06.789000',
        bufferHex: '00017F80FF',
      },
    );

    const connection = await mysql.createConnection(config);
    try {
      const [directRows] = await connection.execute(
        'SELECT ? AS shortString, ? AS intValue, ? AS boolValue',
        ['five!', 9310, true],
      );
      assertJson('direct-connection parameter values', directRows, [
        { shortString: 'five!', intValue: 9310, boolValue: 1 },
      ]);
    } finally {
      await connection.end();
    }

    let rejectedMessage = '';
    try {
      await pool.execute('SELECT ? AS unsupported', [undefined]);
    } catch (error: any) {
      rejectedMessage = String(error && error.message ? error.message : error);
    }
    if (!rejectedMessage.includes('undefined')) {
      throw new Error(
        `undefined bind parameter was not rejected loudly: ${rejectedMessage}`,
      );
    }

    console.log('issue 9310 mysql2 parameter values: OK');
  } finally {
    await pool.end();
  }
}

main();
