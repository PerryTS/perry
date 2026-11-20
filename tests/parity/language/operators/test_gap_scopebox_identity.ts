// Ordinary-object allocation proofs must not be inferred from annotations.
const object = { value: 1 }, other = { value: 1 };
function ordinary() {
  const fixed = { value: 1 };
  const alias = fixed;
  console.log(fixed === alias, fixed === other, fixed !== object);
  const symbol: any = Symbol.for('scopebox');
  console.log(fixed === symbol, fixed === (NaN as any), fixed === (0 as any));
}
ordinary();
function eq(a: any, b: any) { return a === b; }
const grown: any[] = [], alias = grown;
for (let i = 0; i < 100; i++) grown.push(i);
console.log(eq(grown, alias), eq(grown, []), eq(object, object), eq(object, other));
console.log(eq(NaN, NaN), eq(-0, 0), eq(0, -0), eq('scopebox', 'scope' + 'box'));
console.log(eq(Symbol.for('scopebox'), Symbol.for('scopebox')), eq(Symbol('x'), Symbol('x')));
const lied: object = [];
console.log(eq(lied, lied), eq(lied, object));
// A constructor's spelling cannot prove an ordinary object allocation.
class __AnonShape_Lookalike {
  constructor() { return [] as any; }
}
function constructorOverride() {
  const value: any = new __AnonShape_Lookalike(), saved = value;
  for (let i = 0; i < 100; i++) value.push(i);
  console.log(value === saved, value === object);
}
constructorOverride();
