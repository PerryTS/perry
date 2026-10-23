// Wide literal births (#13049 / LIT3): a 48-field literal-shape birth stores
// every slot and then runs one barrier for the whole birth. Covers mixed value
// representations (ints, doubles, strings, objects, functions, arrays, bigints,
// numeric constants in raw lanes), a value whose evaluation collects in the
// middle of the birth, a throwing value, and births kept alive across many
// collections so that the children stored into them must survive. Generated.
let j = 0;
const order: number[] = [];
function churn(i: number): number {
  // Allocates enough that a collection runs while the birth's earlier values
  // are held (evaluated, not yet stored).
  let keep: any[] = [];
  for (let k = 0; k < 4000; k++) keep.push({ k, s: 'c' + k });
  return keep.length + i;
}
function thrower(i: number): number {
  order.push(i);
  if (i % 3 === 0) throw new Error('boom ' + i);
  return i;
}
function make(i: number) {
  j = i;
  return {
    f0: i + 0,
    f1: i + 1.25,
    f2: 's2_' + i,
    f3: { v: i * 3 },
    f4: () => i + 4,
    f5: [i, '5', { w: 5 }],
    f6: 6.5,
    f7: (i & 4) !== 0,
    f8: j % 2 === 0 ? null : undefined,
    f9: BigInt(i) * 9n,
    f10: -0,
    f11: i + 11,
    f12: i + 12.25,
    f13: 's13_' + i,
    f14: { v: i * 14 },
    f15: () => i + 15,
    f16: [i, '16', { w: 16 }],
    f17: 17.5,
    f18: (i & 8) !== 0,
    f19: j % 2 === 0 ? null : undefined,
    f20: BigInt(i) * 20n,
    f21: -0,
    f22: i + 22,
    f23: i + 23.25,
    f24: 's24_' + i,
    f25: { v: i * 25 },
    f26: () => i + 26,
    f27: [i, '27', { w: 27 }],
    f28: 28.5,
    f29: (i & 16) !== 0,
    f30: j % 2 === 0 ? null : undefined,
    f31: BigInt(i) * 31n,
    f32: -0,
    f33: i + 33,
    f34: i + 34.25,
    f35: 's35_' + i,
    f36: { v: i * 36 },
    f37: () => i + 37,
    f38: [i, '38', { w: 38 }],
    f39: 39.5,
    f40: (i & 1) !== 0,
    f41: j % 2 === 0 ? null : undefined,
    f42: BigInt(i) * 42n,
    f43: -0,
    f44: i + 44,
    f45: i + 45.25,
    f46: 's46_' + i,
    f47: { v: i * 47 },
  };
}
function makeGc(i: number) {
  j = i;
  return {
    f0: i + 0,
    f1: i + 1.25,
    f2: 's2_' + i,
    f3: { v: i * 3 },
    f4: () => i + 4,
    f5: [i, '5', { w: 5 }],
    f6: 6.5,
    f7: (i & 4) !== 0,
    f8: j % 2 === 0 ? null : undefined,
    f9: BigInt(i) * 9n,
    f10: -0,
    f11: i + 11,
    f12: i + 12.25,
    f13: 's13_' + i,
    f14: { v: i * 14 },
    f15: () => i + 15,
    f16: [i, '16', { w: 16 }],
    f17: 17.5,
    f18: (i & 8) !== 0,
    f19: j % 2 === 0 ? null : undefined,
    f20: BigInt(i) * 20n,
    f21: -0,
    f22: i + 22,
    f23: i + 23.25,
    f24: 's24_' + i,
    f25: { v: i * 25 },
    f26: () => i + 26,
    f27: [i, '27', { w: 27 }],
    f28: 28.5,
    f29: (i & 16) !== 0,
    f30: j % 2 === 0 ? null : undefined,
    f31: BigInt(i) * 31n,
    f32: -0,
    f33: churn(i),
    f34: i + 34.25,
    f35: 's35_' + i,
    f36: { v: i * 36 },
    f37: () => i + 37,
    f38: [i, '38', { w: 38 }],
    f39: 39.5,
    f40: (i & 1) !== 0,
    f41: j % 2 === 0 ? null : undefined,
    f42: BigInt(i) * 42n,
    f43: -0,
    f44: i + 44,
    f45: i + 45.25,
    f46: 's46_' + i,
    f47: { v: i * 47 },
  };
}
function makeThrow(i: number) {
  j = i;
  return {
    f0: i + 0,
    f1: i + 1.25,
    f2: 's2_' + i,
    f3: { v: i * 3 },
    f4: () => i + 4,
    f5: [i, '5', { w: 5 }],
    f6: 6.5,
    f7: (i & 4) !== 0,
    f8: j % 2 === 0 ? null : undefined,
    f9: BigInt(i) * 9n,
    f10: -0,
    f11: i + 11,
    f12: i + 12.25,
    f13: 's13_' + i,
    f14: { v: i * 14 },
    f15: () => i + 15,
    f16: [i, '16', { w: 16 }],
    f17: 17.5,
    f18: (i & 8) !== 0,
    f19: j % 2 === 0 ? null : undefined,
    f20: BigInt(i) * 20n,
    f21: -0,
    f22: i + 22,
    f23: i + 23.25,
    f24: 's24_' + i,
    f25: { v: i * 25 },
    f26: () => i + 26,
    f27: [i, '27', { w: 27 }],
    f28: 28.5,
    f29: (i & 16) !== 0,
    f30: j % 2 === 0 ? null : undefined,
    f31: BigInt(i) * 31n,
    f32: -0,
    f33: thrower(i),
    f34: i + 34.25,
    f35: 's35_' + i,
    f36: { v: i * 36 },
    f37: () => i + 37,
    f38: [i, '38', { w: 38 }],
    f39: 39.5,
    f40: (i & 1) !== 0,
    f41: j % 2 === 0 ? null : undefined,
    f42: BigInt(i) * 42n,
    f43: -0,
    f44: i + 44,
    f45: i + 45.25,
    f46: 's46_' + i,
    f47: { v: i * 47 },
  };
}
function sum(o: any): number {
  let s = 0;
  s += o.f0;
  s += o.f1;
  s += o.f2.length;
  s += o.f3.v;
  s += o.f4();
  s += o.f5[0] + o.f5[2].w;
  s += o.f6;
  s += (o.f7 ? 1 : 0);
  s += (o.f8 === null ? 1 : 0);
  s += Number(o.f9 % 1000n);
  s += (Object.is(o.f10, -0) ? 1 : 0);
  s += o.f11;
  s += o.f12;
  s += o.f13.length;
  s += o.f14.v;
  s += o.f15();
  s += o.f16[0] + o.f16[2].w;
  s += o.f17;
  s += (o.f18 ? 1 : 0);
  s += (o.f19 === null ? 1 : 0);
  s += Number(o.f20 % 1000n);
  s += (Object.is(o.f21, -0) ? 1 : 0);
  s += o.f22;
  s += o.f23;
  s += o.f24.length;
  s += o.f25.v;
  s += o.f26();
  s += o.f27[0] + o.f27[2].w;
  s += o.f28;
  s += (o.f29 ? 1 : 0);
  s += (o.f30 === null ? 1 : 0);
  s += Number(o.f31 % 1000n);
  s += (Object.is(o.f32, -0) ? 1 : 0);
  s += o.f33;
  s += o.f34;
  s += o.f35.length;
  s += o.f36.v;
  s += o.f37();
  s += o.f38[0] + o.f38[2].w;
  s += o.f39;
  s += (o.f40 ? 1 : 0);
  s += (o.f41 === null ? 1 : 0);
  s += Number(o.f42 % 1000n);
  s += (Object.is(o.f43, -0) ? 1 : 0);
  s += o.f44;
  s += o.f45;
  s += o.f46.length;
  s += o.f47.v;
  return s;
}
const kept: any[] = [];
let total = 0;
for (let i = 0; i < 30000; i++) {
  const o = make(i);
  total += sum(o) % 1000003;
  if (i % 5 === 0) kept.push(o);
  // Garbage between births so births land in different nursery states.
  const g: any[] = [];
  for (let k = 0; k < 20; k++) g.push({ k, t: 'g' + k });
}
let keptSum = 0;
for (const o of kept) keptSum = (keptSum + sum(o)) % 1000003;
console.log('births', total, 'kept', kept.length, keptSum);
let gcSum = 0;
const gcKept: any[] = [];
for (let i = 0; i < 40; i++) { const o = makeGc(i); gcKept.push(o); gcSum += sum(o) % 1000003; }
let gcKeptSum = 0;
for (const o of gcKept) gcKeptSum = (gcKeptSum + sum(o)) % 1000003;
console.log('gc mid-birth', gcSum, gcKeptSum, Object.keys(gcKept[7]).length);
let thrown = 0;
let born = 0;
let last: any = undefined;
for (let i = 0; i < 12; i++) {
  try { last = makeThrow(i); born++; } catch (e) { thrown++; }
}
console.log('throw', thrown, born, order.join(','), last.f33, sum(last) % 1000003);
const one = make(7);
console.log(Object.keys(one).length, Object.keys(one).slice(30, 36).join(','), JSON.stringify([one.f0, one.f2, one.f6, one.f33, one.f47]));

