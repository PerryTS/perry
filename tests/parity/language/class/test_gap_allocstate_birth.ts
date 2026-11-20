// Complete births, scalar/boxed inputs, and constructors with observable work.
class Pair {
  left: any;
  right: any;
  constructor(left: any, right: any) { this.left = left; this.right = right; }
}
const child = { value: 7 };
const a = new Pair(child, null);
console.log(a.left === child, a.right, Object.keys(a).join(','));
const b = new Pair('s', false);
console.log(b.left, b.right, Object.keys(b).join(','));
class Numbers {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
}
const odd = new Numbers('boxed' as any, NaN);
console.log(odd.x, Number.isNaN(odd.y));
let escaped: any = null;
let observations = '';
function observe(p: any): void {
  escaped = p;
  observations += String(p.right) + ',';
  for (let i = 0; i < 3000; i++) new Pair({ value: i }, null);
}
class Observed {
  left: any;
  right: any;
  constructor(left: any, right: any) {
    this.left = left;
    observe(this);
    this.right = right;
  }
}
const observed = new Observed(child, { value: 9 });
console.log(observations, escaped === observed, observed.left.value, observed.right.value);
class Base {
  left: any;
  constructor(left: any) { this.left = left; }
}
class Derived extends Base {
  right: any;
  constructor(left: any, right: any) { super(left); this.right = right; }
}
const derived = new Derived(child, b);
console.log(derived.left === child, derived.right === b, derived instanceof Base);
class Override {
  value: any;
  constructor(value: any) { this.value = value; return { value: 'override' } as any; }
}
console.log(new Override(3).value);

// Five boxed fields use the shared fill and the fresh NoCollect prologue.
class Wide {
  a: any; b: any; c: any; d: any; e: any;
  constructor(a: any, b: any, c: any, d: any, e: any) {
    this.a = a; this.b = b; this.c = c; this.d = d; this.e = e;
  }
}
const kept: Wide[] = [];
for (let i = 0; i < 12000; i++) {
  const kid = { value: i };
  kept[i % 16] = new Wide(kid, a, b, observed, derived);
}
let wideSum = 0;
for (let i = 0; i < 16; i++) {
  const w = kept[i];
  wideSum += w.a.value;
  if (w.b !== a || w.c !== b || w.d !== observed || w.e !== derived) console.log('lost-wide-child');
}
console.log('wide', wideSum);
