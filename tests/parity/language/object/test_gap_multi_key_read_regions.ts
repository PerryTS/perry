// #12328: the same helper must agree with Node on region hits and all bails.
function cellsConflict(cell1: any, cell2: any) {
  let yMin1 = cell1.y;
  let yMax1 = cell1.y - 1 + (cell1.rowSpan || 1);
  let yMin2 = cell2.y;
  let yMax2 = cell2.y - 1 + (cell2.rowSpan || 1);
  let yConflict = !(yMin1 > yMax2 || yMin2 > yMax1);
  let xMin1 = cell1.x;
  let xMax1 = cell1.x - 1 + (cell1.colSpan || 1);
  let xMin2 = cell2.x;
  let xMax2 = cell2.x - 1 + (cell2.colSpan || 1);
  let xConflict = !(xMin1 > xMax2 || xMin2 > xMax1);
  return yConflict && xConflict;
}
function check(label: string, a: any, b: any) {
  console.log(label, cellsConflict(a, b), cellsConflict(a, b), cellsConflict(a, b));
}
const a: any = { x: 1, y: 2, rowSpan: 2, colSpan: 3 };
const b: any = { x: 2, y: 3, rowSpan: 1, colSpan: 1 };
check('F64', a, b);
check('absent', { x: 1, y: 2 }, b);
a.x = '2';
check('string after prime', a, b);
a.x = 1;
check('generalized Any', a, b);
check('NaN', { x: NaN, y: NaN, rowSpan: NaN, colSpan: NaN }, b);
check('negative zero', { x: -0, y: -0, rowSpan: -0, colSpan: -0 }, b);
check('infinity', { x: Infinity, y: -Infinity, rowSpan: 0, colSpan: 0 }, b);
let reads = '';
const getter: any = { x: 1, rowSpan: 1, colSpan: 1 };
Object.defineProperty(getter, 'y', { configurable: true, get() { reads += 'y'; return 2; } });
check('getter', getter, b);
console.log('getter order', reads);
Object.defineProperty(a, 'rowSpan', { configurable: true, get() { reads += 'r'; return 2; } });
check('getter after hit', a, b);
console.log('getter mutation order', reads);
const inherited: any = { x: 1, y: 2, colSpan: 3 };
Object.setPrototypeOf(inherited, { rowSpan: 2 });
check('inherited', inherited, b);
Object.setPrototypeOf(inherited, { rowSpan: 0 });
check('changed proto', inherited, b);
const deleted: any = { x: 1, y: 2, rowSpan: 2, colSpan: 3 };
check('before delete', deleted, b);
delete deleted.rowSpan;
Object.setPrototypeOf(deleted, { rowSpan: 4 });
check('delete and proto', deleted, b);
class Cell {
  x: any; y: any; rowSpan: any; colSpan: any;
  constructor() { this.x = null; this.y = null; this.rowSpan = 1; this.colSpan = 1; }
}
const real = new Cell();
real.x = 2; real.y = 3;
check('null birth Any', real, b);
// A failed Number proof must run ToPrimitive only in the original order.
const coercible: any = { x: 1, y: 2, rowSpan: 1, colSpan: 1 };
coercible.y = { valueOf() { reads += 'v'; b.y = 100; return 2; } };
check('coercion changes receiver', coercible, b);
console.log('coercion order', reads);
// Refinement control: typed string reads keep their initializer-derived type.
function strings(o: { a: string, b: string }) {
  let x: any = o.a;
  let y: any = o.b;
  return x.length + y.length + x.charCodeAt(0) + y.charCodeAt(0);
}
console.log('refined strings', strings({ a: 'ab', b: 'cd' }));
// Negative admission control: explicit calls remain in source order.
function withCall(o: any) {
  let x = o.x;
  let y = o.y + (() => { o.x = 17; reads += 'c'; return 1; })();
  let z = o.x;
  return x + y + z;
}
console.log('call control', withCall({ x: 1, y: 2 }), reads);
