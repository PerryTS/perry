class ArrayRow {
  x: number;
  y: number;
  constructor(x: number) { this.x = x; this.y = 3; }
}

function rows(): ArrayRow[] {
  const result: ArrayRow[] = [];
  for (let i = 0; i < 8; i++) result.push(new ArrayRow(i));
  return result;
}

function product(a: ArrayRow[], n: number): number {
  let sum = 0;
  for (let i = 0; i < n; i++) {
    const r = a[i & 7];
    sum += r.x * r.y + 7;
  }
  return sum;
}

console.log('plain', product(rows(), 32));
console.log('fractional', product(rows(), 3.5));
console.log('empty', product(rows(), 0));
console.log('string-bound', product(rows(), '3' as any));
let boundReads = 0;
console.log('coercing-bound', product(rows(), {
  valueOf() { boundReads++; return 3; }
} as any), boundReads);

const strings = rows();
(strings[2] as any).x = '12';
(strings[5] as any).y = '4';
console.log('representation', product(strings, 32));

function total(a: ArrayRow[], n: number): any {
  let sum: any = 0;
  for (let i = 0; i < n; i++) {
    const r = a[i & 7];
    sum += r.x + r.y;
  }
  return sum;
}
// A failed row proof may turn the accumulator into a string. The next
// otherwise matching row must not reuse the former Number admission.
console.log('string-accumulator', total(strings, 16));

const reordered = rows();
reordered[2] = { y: 4, x: 12 } as ArrayRow;
console.log('other-shape', product(reordered, 32));

let gets = 0;
const accessor = rows();
Object.defineProperty(accessor[3], 'y', {
  get() { gets++; accessor[3].x = 40; return 5; }, configurable: true
});
console.log('accessor', product(accessor, 32), gets);

const indexed = rows();
const saved = indexed[2];
let indexGets = 0;
Object.defineProperty(indexed, '2', {
  get() { indexGets++; return saved; }, configurable: true
});
console.log('index-accessor', product(indexed, 32), indexGets);

let proxyGets = 0;
const proxied = new Proxy(rows(), {
  get(target, key, receiver) {
    if (key === '2') proxyGets++;
    return Reflect.get(target, key, receiver);
  }
});
console.log('proxy', product(proxied, 32), proxyGets);

const hole = rows();
delete hole[2];
try { console.log(product(hole, 32)); }
catch (e) { console.log('hole', e instanceof TypeError); }
try { console.log(product(rows().slice(0, 2), 32)); }
catch (e) { console.log('short', e instanceof TypeError); }

function changing(a: ArrayRow[]): number {
  let sum = 0;
  for (let i = 0; i < 32; i++) {
    const r = a[i & 7];
    sum += r.x * r.y + 7;
    if (i === 3) a[0] = new ArrayRow(20);
  }
  return sum;
}
console.log('changing', changing(rows()));


class BoxedRow {
  x: any;
  y: any;
  constructor(x: any, y: any) { this.x = x; this.y = y; }
}
const differentlyRepresented: ArrayRow[] = [];
for (let i = 0; i < 8; i++) differentlyRepresented.push(new BoxedRow(2, '3') as any);
console.log('nominated-shape-mismatch', product(differentlyRepresented, 32));
