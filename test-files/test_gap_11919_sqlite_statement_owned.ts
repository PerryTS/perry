// #11919: a StatementSync owns the statement its prepare() compiled, as
// node's does: the authorizer runs at prepare() only, expandedSQL reads the
// last bindings, columns() runs nothing; own getters are born on every
// statement; an own property shadows a prototype method; statements left
// over after close() are finalized once.
import { DatabaseSync } from 'node:sqlite';

const db = new DatabaseSync(':memory:');
let auth = 0;
db.setAuthorizer(() => {
  auth++;
  return 0;
});
db.exec('CREATE TABLE t(x INTEGER, y TEXT)');
auth = 0;
const ins = db.prepare('INSERT INTO t VALUES (?, ?)');
const afterPrepare = auth;
ins.run(1, 'a');
ins.run(2, 'b');
ins.run(3, 'c');
console.log('authorizer: prepare', afterPrepare > 0, 'runs', auth - afterPrepare);
console.log('expanded', ins.expandedSQL);
console.log('source', ins.sourceSQL);
try {
  ins.run(1, 2, 3);
} catch (e) {
  console.log('bind error', (e as any).code);
}
console.log('after bind error', JSON.stringify(ins.run(4, 'd')));

const sel = db.prepare('SELECT x, y FROM t WHERE x >= ?');
auth = 0;
console.log('columns', JSON.stringify(sel.columns().map((c: any) => c.name)), 'auth', auth);
console.log('all', JSON.stringify(sel.all(2)));
console.log('get', JSON.stringify(sel.get(3)), 'expanded', sel.expandedSQL);
console.log('iterate', JSON.stringify([...sel.iterate(4)]), 'runs', auth);

const stmts: any[] = [];
for (let i = 0; i < 50; i++) stmts.push(db.prepare(`SELECT ${i} AS n`));
console.log(
  'own getters',
  stmts.every((s, i) => s.sourceSQL === `SELECT ${i} AS n` && s.expandedSQL === `SELECT ${i} AS n` && s.get().n === i),
);
const d = Object.getOwnPropertyDescriptor(stmts[7], 'expandedSQL')!;
console.log('descriptor', typeof d.get, d.set, d.enumerable, d.configurable);
console.log('keys', JSON.stringify(Object.keys(stmts[7])), JSON.stringify(Object.getOwnPropertyNames(stmts[8])));

Object.defineProperty(stmts[3], 'get', { value: () => 'own get', configurable: true });
console.log('own method', stmts[3].get(), JSON.stringify(stmts[4].get()));

const inner = db.prepare('SELECT count(*) AS c FROM t');
db.function('cnt', () => (inner.get() as any).c);
const outer = db.prepare('SELECT cnt() AS a, cnt() AS b');
console.log('nested', JSON.stringify(outer.get()), JSON.stringify(outer.get()));

db.close();
try {
  sel.get(1);
} catch (e) {
  console.log('after close', (e as any).code);
}
let garbage = 0;
for (let i = 0; i < 300; i++) {
  const other = new DatabaseSync(':memory:');
  for (let j = 0; j < 5; j++) other.prepare(`SELECT ${j}`);
  if (i % 2 === 0) other.close();
  garbage += new Array(2000).fill(i).length;
}
console.log('left over statements finalized once', garbage);
