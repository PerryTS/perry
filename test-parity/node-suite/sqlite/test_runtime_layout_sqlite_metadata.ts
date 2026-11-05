import { DatabaseSync } from 'node:sqlite';
const db = new DatabaseSync(':memory:');
const stmt = db.prepare('SELECT 1 AS first, 2 AS second');
const columns = stmt.columns();
for (const column of columns) {
  console.log(Object.keys(column).join(','));
  for (const key of Object.keys(column)) {
    const d = Object.getOwnPropertyDescriptor(column, key)!;
    console.log(key, d.writable, d.enumerable, d.configurable);
  }
  console.log(Object.getPrototypeOf(column) === Object.prototype, column.name, column.column, column.database, column.table, column.type);
  column.extra = 29;
  console.log(Object.keys(column).join(','), column.extra);
}
db.close();
console.log(columns[0].name, columns[1].name);
