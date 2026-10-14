// Number::remainder (`%`) on exact integers beyond i32 but within ±2^53, and
// the edges the integer fast path must preserve: the dividend's sign on a zero
// result (-0), NaN for a zero divisor, and fmod for everything non-integral.
function show(label: string, v: number): void {
  console.log(label, Object.is(v, -0) ? "-0" : String(v));
}

// Values kept in `let`s typed as number so codegen cannot fold them.
let lcgSeed = 12345678;
let lcgMod = 2147483647;
let big = 9007199254740992; // 2^53
let neg = -9007199254740992;
let two = 2;
let three = 3;
let zero = 0;
let negZero = -0;
let half = 2.5;

// A Park–Miller LCG step overflows i32 but stays exact in i53.
let s = lcgSeed;
for (let i = 0; i < 5; i++) {
  s = (s * 48271) % lcgMod;
  show("lcg", s);
}

show("2^53 % 3", big % three);
show("-2^53 % 7", neg % 7);
show("2^53 % 2^53", big % big);
show("(2^53-1) % -2", (big - 1) % -two);
show("-(2^40) % 2^20", -(2 ** 40) % (2 ** 20));

// Zero results take the dividend's sign.
show("-4 % 2", -4 % two);
show("4 % -2", 4 % -two);
show("-4 % -2", -4 % -two);
show("-0 % 5", negZero % 5);
show("0 % -5", zero % -5);
show("-3000000000 % 1000000000", -3000000000 % 1000000000);

// Nonzero results take the dividend's sign.
show("-7 % 3", -7 % three);
show("7 % -3", 7 % -three);
show("-7000000000 % 3", -7000000000 % three);

// Zero divisor and non-finite / non-integral operands.
show("5 % 0", 5 % zero);
show("5 % -0", 5 % negZero);
show("0 % 0", zero % zero);
show("NaN % 3", NaN % three);
show("Infinity % 3", Infinity % three);
show("3 % Infinity", three % Infinity);
show("-3 % -Infinity", -three % -Infinity);
show("5.5 % 2", 5.5 % two);
show("5 % 2.5", 5 % half);
show("2^54 % 3", big * 2 % three);
show("1e300 % 7", 1e300 % 7);

// The dynamic path: operands typed as unknown values.
const vals: unknown[] = [103524123456789, -4, 5, -0];
const divs: unknown[] = [2147483647, 2, 0, 5];
for (let i = 0; i < vals.length; i++) {
  show("dyn", (vals[i] as number) % (divs[i] as number));
}
