// ToNumber throws a TypeError for a BigInt, including one that ToPrimitive
// returns for an object operand; Number() and the operators that use
// ToNumeric keep converting or keep BigInt semantics.

function show(label: string, run: () => unknown): void {
  try {
    console.log(label, String(run()));
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}

const one: any = 1n;
const two: any = 2n;
const bigFromValueOf: any = { valueOf: () => 1n };

show("slice", () => [1, 2, 3].slice(one));
show("at", () => [0, 1, 2].at(one));
show("charAt", () => "abc".charAt(one));
show("repeat", () => "x".repeat(two));
show("isNaN", () => isNaN(one));
show("isFinite", () => isFinite(one));
show("parseInt radix", () => parseInt("10", one));
show("Math.trunc object", () => Math.trunc(bigFromValueOf));
show("Math.max", () => Math.max(one));
show("Array.from length object", () => Array.from({ length: { valueOf: () => 2n } } as any).length);
show("sort comparator", () => [2, 1].sort(() => one).join());
show("toString radix", () => (10).toString(one));
show("toFixed", () => (1.5).toFixed(one));
show("new Date", () => new Date(one).getTime());
show("Float64Array store", () => {
  const a = new Float64Array(1);
  a[0] = one;
  return a[0];
});
show("Intl.PluralRules select", () => new Intl.PluralRules("en").select(one));

show("Number", () => Number(one));
show("Number object", () => Number(bigFromValueOf));
show("new Number", () => new Number(one).valueOf());
show("map Number", () => [one].map(Number)[0]);
show("Number value", () => {
  const N: any = Number;
  return N(one);
});
show("Number.call", () => Number.call(null, one));
show("NumberFormat format", () => new Intl.NumberFormat("en").format(one));
show("NumberFormat formatRange", () => new Intl.NumberFormat("en").formatRange(one, two));

show("unary plus", () => +one);
show("unary minus", () => -one);
show("bitwise not", () => ~one);
show("increment", () => {
  let v: any = 1n;
  v++;
  return v;
});
show("mixed multiply", () => one * 2);
show("less than", () => one < 2);
show("loose equal", () => one == 1);
show("sum", () => one + two);
show("String", () => String(one));
show("template", () => `${one}`);
show("JSON space", () => JSON.stringify({ a: 1 }, null, one));
show("new Array", () => new Array(one).length);
show("Map key", () => new Map([[one, "a"]]).get(1n));
const xs: any[] = [1n, 2n];
show("element less than", () => xs[0] < 2);
show("element greater than", () => 3 > xs[1]);
show("element sum", () => xs[0] + xs[1]);
show("element mixed", () => xs[0] * 2);
show("sort bigints", () => [3n, 1n, 2n].sort().join());
