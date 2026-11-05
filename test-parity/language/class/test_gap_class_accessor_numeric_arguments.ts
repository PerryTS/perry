class Bits {
  data = 0;
  set input(x: number) { this.data = x & 255; }
  get output() { return this.data * 3 + 7; }
}
class ClosedBits {
  data = 0;
  set input(x: number) { this.data = x & 255; }
  get output() { return this.data * 3 + 7; }
}
function small(n: number) {
  const b = new ClosedBits(); let total = 0;
  for (let i = 0; i < n; i++) { b.input = i; total += b.output; }
  return total;
}
function dynamic(b: Bits, value: any) { b.input = value; return b.output; }
console.log('bounded', small(1000));
const b = new Bits();
for (const value of [1e20, -1e20, Infinity, NaN, '17']) {
  console.log('dynamic', dynamic(b, value));
}
console.log('coercion', dynamic(b, { valueOf() { return 9; } }));
try { dynamic(b, 1n); } catch (e) { console.log('mixed', e instanceof TypeError); }
class ParentBits {
  data = 0;
  set input(x: number) { this.data = x & 255; }
  get output() { return this.data * 3 + 7; }
}
class Child extends ParentBits {}
function inherited(n: number) {
  const b = new Child(); let total = 0;
  for (let i = 0; i < n; i++) { b.input = i; total += b.output; }
  return total;
}
console.log('inherited', inherited(1000));
class Liar {
  data: number = 's' as any;
  get output(): number { return this.data; }
}
function lying() {
  const b = new Liar(); let total: any = 0;
  for (let i = 0; i < 4; i++) total += b.output;
  return total;
}
console.log('lying', lying());
