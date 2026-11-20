// JSON.stringify must format numbers with ECMAScript Number::toString
// (spec 6.1.6.1.20) — the same notation as String(n) and Node — rather than
// `ryu`'s shortest-digit notation. That means fixed notation for an exponent
// in -6..=20 (so 1e20 is `100000000000000000000`, 1e-6 is `0.000001`) and
// exponential with an `e+`/`e-` sign otherwise (1e21 is `1e+21`).

function show(label: string, value: any) {
  console.log(label + " = " + value);
}

show("1e20", JSON.stringify(1e20));
show("1e21", JSON.stringify(1e21));
show("1.5e21", JSON.stringify(1.5e21));
show("1e-6", JSON.stringify(1e-6));
show("1e-7", JSON.stringify(1e-7));
show("1e100", JSON.stringify(1e100));
show("1e-300", JSON.stringify(1e-300));
show("bignum", JSON.stringify(123456789012345680000));
show("obj", JSON.stringify({ a: 1e20, b: 1e-6, c: 3.14, d: -2.5, e: 1e21 }));
show("arr", JSON.stringify([1e20, 1e-7, 1e100, 0.000001, 1.5e21]));
show("pretty", JSON.stringify({ x: 1e20, y: 1e-6 }, null, 1).replace(/\s+/g, " "));
show("ints", JSON.stringify([0, -0, 1, -1, 100, 1000000, 999999999999999, 1e15, 1e16]));
show("fracs", JSON.stringify([0.1, 0.2, 0.30000000000000004, 3.14, -2.5]));
