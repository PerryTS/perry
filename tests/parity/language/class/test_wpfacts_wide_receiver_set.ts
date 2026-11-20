// Check mode (`PERRY_VERIFY_FACTS=1`): a read whose receiver set holds more
// than four classes is proven when every class agrees on the key's slot. The
// witness must admit a receiver by membership in the site's whole proven
// set: a receiver of the sixth class is as proven as one of the first.
// Every case prints what node prints.
class C1 { v = 1; k = "c1"; }
class C2 { v = 2; k = "c2"; m = 0; }
class C3 { v = 3; k = "c3"; n = true; }
class C4 { v = 4; k = "c4"; }
class C5 { v = 5; k = "c5"; o = "x"; }
class C6 { v = 6; k = "c6"; p = 6; q = 7; }
class C7 { v = 7; k = "c7"; }
type Any7 = C1 | C2 | C3 | C4 | C5 | C6 | C7;
function sumV(xs: Any7[]): number {
  let s = 0;
  for (let i = 0; i < xs.length; i++) s += xs[i].v;
  return s;
}
function keys(xs: Any7[]): string {
  let s = "";
  for (const x of xs) s += x.k;
  return s;
}
const xs: Any7[] = [];
for (let i = 0; i < 70; i++) {
  switch (i % 7) {
    case 0: xs.push(new C1()); break;
    case 1: xs.push(new C2()); break;
    case 2: xs.push(new C3()); break;
    case 3: xs.push(new C4()); break;
    case 4: xs.push(new C5()); break;
    case 5: xs.push(new C6()); break;
    default: xs.push(new C7());
  }
}
console.log(sumV(xs), keys(xs.slice(0, 7)));
console.log(sumV(xs.slice(4)), sumV([new C7(), new C6(), new C5()]));
