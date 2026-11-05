// Actual Number operations, erased declarations and pointer stores share the
// same result contract. Keep coercion order, BigInt throws and IEEE values.
class Cell {
  x: number;
  constructor(x: number) { this.x = x; }
}
function show(x: any): string { return Object.is(x, -0) ? "-0" : String(x); }
function step(o: Cell): any { return (o.x + 1) % 10; }
const values: any[] = [0, -1, -11, 0.5, -0, NaN, Infinity, -Infinity, 2147483648, "2", null, undefined, true];
const target: any = { value: 0 };
for (let i = 0; i < values.length; i++) {
  const o = new Cell(values[i]);
  target.value = step(o);
  console.log(i + ":" + show(target.value));
}
let effects = "";
const coercible: any = { valueOf() { effects += "v"; return 6; } };
target.value = step(new Cell(coercible));
console.log(show(target.value) + ":" + effects);
function rem(a: any, b: any): any { return a % b; }
console.log(show(rem(23n, 7n)));
try { console.log(rem(3n, 2)); } catch (e) { console.log(e instanceof TypeError); }
try { console.log(step(new Cell(3n as any))); } catch (e) { console.log(e instanceof TypeError); }
// Dynamic arithmetic results may still be pointer-bearing BigInts or strings.
target.value = rem(9007199254740993n, 97n);
console.log(typeof target.value + ":" + String(target.value));
const src: any = { value: "ab" };
target.value = src.value + "c";
src.value += "d";
console.log(target.value + ":" + src.value);
const setter: any = { set value(v: any) { effects += "s" + show(v); } };
setter.value = step(new Cell(9));
console.log(effects);

function declaredRem(a: number, b: number): any { return a % b; }
console.log(String(declaredRem(23n as any, 7n as any)));
function declaredAdd(a: number, b: number): any { return a + b; }
target.value = declaredAdd("a" as any, "b" as any);
console.log(target.value);
