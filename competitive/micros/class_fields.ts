// Fixed-order declared fields; retained instances prevent scalar replacement.
class Fields1 {
  f0 = 1;
}
class Fields8 {
  f0 = 1;
  f1 = 2;
  f2 = 3;
  f3 = 4;
  f4 = 5;
  f5 = 6;
  f6 = 7;
  f7 = 8;
}
class Fields16 {
  f0 = 1;
  f1 = 2;
  f2 = 3;
  f3 = 4;
  f4 = 5;
  f5 = 6;
  f6 = 7;
  f7 = 8;
  f8 = 9;
  f9 = 10;
  f10 = 11;
  f11 = 12;
  f12 = 13;
  f13 = 14;
  f14 = 15;
  f15 = 16;
}
class Fields32 {
  f0 = 1;
  f1 = 2;
  f2 = 3;
  f3 = 4;
  f4 = 5;
  f5 = 6;
  f6 = 7;
  f7 = 8;
  f8 = 9;
  f9 = 10;
  f10 = 11;
  f11 = 12;
  f12 = 13;
  f13 = 14;
  f14 = 15;
  f15 = 16;
  f16 = 17;
  f17 = 18;
  f18 = 19;
  f19 = 20;
  f20 = 21;
  f21 = 22;
  f22 = 23;
  f23 = 24;
  f24 = 25;
  f25 = 26;
  f26 = 27;
  f27 = 28;
  f28 = 29;
  f29 = 30;
  f30 = 31;
  f31 = 32;
}
const n = Number(process.argv[2] || '200000');
const fields = Number(process.argv[3] || '16');
const retained: any[] = new Array(256);
function construct(): any {
  if (fields === 1) return new Fields1();
  if (fields === 8) return new Fields8();
  if (fields === 32) return new Fields32();
  return new Fields16();
}
let sum = 0;
for (let i = 0; i < n; i++) {
  const value = construct();
  retained[i & 255] = value;
  sum += value.f0;
}
console.log(sum, Object.keys(retained[0]).length);
