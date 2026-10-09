// One generic read site per mechanism; deterministic output, also run on Node.
const N = Number(process.argv[2] || 200000);
function own(o: any) { return o.value; }
function inherited(o: any) { return o.value; }
function absent(o: any) { return o.missing; }
function method(o: any) { return o.method(); }
function prototype(o: any) { return Object.getPrototypeOf(o).value; }
function functionOwn(f: any) { return f.prototype.value; }
function Factory() {}
(Factory as any).prototype.value = 17;
const holder: any = { value: 7, method() { return this.local; } };
const mid = Object.create(holder);
const plain: any = { value: 3 };
const child: any = Object.create(mid);
child.local = 11;
const shapes: any[] = [];
for (let i = 0; i < 64; i++) {
  const o: any = Object.create(holder);
  o["key" + i] = i;
  o.local = i;
  shapes.push(o);
}
let sum = 0, misses = 0;
for (let i = 0; i < N; i++) {
  sum += own(plain) + inherited(child) + method(child) + functionOwn(Factory);
  if (absent(child) === undefined) misses++;
}
console.log("stable", sum, misses);
sum = 0;
for (let i = 0; i < N; i++) {
  const o = shapes[i % shapes.length];
  sum += inherited(o) + method(o) + prototype(o);
}
console.log("wide", sum);
