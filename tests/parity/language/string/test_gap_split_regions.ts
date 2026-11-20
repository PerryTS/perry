// Structural function splitting (split_regions): one function large enough to be
// split (more than 4000 HIR nodes) whose statements cover the cases a region
// boundary must preserve: typeof-guarded arms (typed regions), state shared with
// closures (closure regions), hoisted declarations used before and after their
// position, var hoisting out of a branch, labeled
// control flow, exceptions crossing a region, switch fallthrough, classes declared
// inside the function (with and without captured locals), destructuring, closures
// in loops, live-outs (one and several, typed and not) and generators.

function big(x: any, y: any, items: any, dyn: any): any[] {
  const out: any[] = [];
  let h = 17;
  // typeof-guarded arms
  if (typeof x === "string") {
    const up = x.toUpperCase();
    const parts = up.split(" ");
    let n = 0;
    for (let j = 0; j < parts.length; j++) { n += parts[j].length * (j + 1); }
    h = (h * 31 + n + up.charCodeAt(0)) % 1000003;
    out.push(up, parts.length, n);
  }
  if (typeof y === "number") {
    const sq = y * y;
    let t = 0;
    for (let k = 0; k < 10; k++) { t += (sq + k) % 97; }
    h = (h * 31 + t) % 1000003;
    out.push(sq, t);
  }
  // state shared with closures, written by both
  let count = 0;
  const inc = (d: number) => { count += d; return count; };
  let trail = "";
  for (let i = 0; i < 4; i++) { trail += String(inc(i)) + ","; }
  count = count * 2;
  out.push(trail, count, inc(1));
  // a hoisted function used before its declaration, reading a later const
  out.push(early(3));
  const factor = 7;
  function early(v: number): number { return v * 2 + count; }
  function scaled(v: number): number { return v * factor; }
  out.push(early(4) + factor, scaled(2));
  // a function declaration whose binding is used only far below
  function late(v: number): number { return v + factor + count; }
  // var hoisted out of a branch
  if (h % 2 === 0) { var hv = "even"; } else { var hv = "odd"; }
  out.push(hv);
  // labeled control flow
  let lab = 0;
  outer: for (let i = 0; i < 5; i++) {
    for (let j = 0; j < 5; j++) {
      if (j === 3) continue outer;
      if (i * j > 6) break outer;
      lab += i + j;
    }
  }
  out.push(lab);
  // exceptions
  let caught = "";
  try {
    caught += "a";
    JSON.parse(dyn.bad);
    caught += "b";
  } catch (e: any) {
    caught += "c:" + String(e && e.name);
  } finally {
    caught += ":f";
  }
  out.push(caught);
  // switch with fallthrough
  let sw = 0;
  switch (h % 4) {
    case 0: sw += 1;
    case 1: sw += 10; break;
    case 2: sw += 100;
    default: sw += 1000;
  }
  out.push(sw);
  // classes declared here, one capturing a local
  let made = 0;
  class Plain { v: number; constructor(v: number) { this.v = v * 2; } get dbl() { return this.v * 2; } }
  class Counting { v: number; constructor(v: number) { made++; this.v = v + made; } }
  const p1 = new Plain(5);
  const c1 = new Counting(1);
  const c2 = new Counting(2);
  out.push(p1.dbl, c1.v, c2.v, made);
  // destructuring with defaults and rest
  const src: any = { a: 1, b: [2, 3, 4], c: { d: "e" } };
  const { a: da, b: [db0, ...dbr], c: { d: dd }, z: dz = "zz" } = src;
  out.push(da, db0, dbr.length, dd, dz);
  // closures in a loop, each with its own binding
  const fns: any[] = [];
  for (let k = 0; k < 3; k++) { fns.push(() => k * factor); }
  out.push(fns.map((f) => f()).join("|"));
  // live-outs: several, written in one place, read much later
  let la = 1, lb = "s", lc: any = null;
  la = la + h % 7; lb = lb + la; lc = { la, lb };
  // a generator declared and used here
  function* gen(n: number) { for (let i = 0; i < n; i++) yield i * i; }
  let gs = 0;
  for (const v of gen(4)) gs += v;
  out.push(gs);
  // a closure created here that outlives the statements around it
  const keep = (() => { const base = h % 11; return (v: number) => v + base; })();
  // block 0
  {
    const s0 = String(h) + "0";
    let r0 = "";
    for (let m = 0; m < s0.length; m++) { r0 = s0[m] + r0; }
    const arr0 = [h % 3, h % 5, h % 7, 0];
    let acc0 = 0;
    for (const v of arr0) { acc0 += v * (v + 1); }
    const obj0: any = { k: 0, s: r0, n: acc0 };
    h = (h * 31 + obj0.n + obj0.s.length + (dyn["k0"] | 0)) % 1000003;
    if (typeof dyn["f0"] === "function") { h = (h + dyn["f0"](0)) % 1000003; }
  }
  // block 1
  {
    const s1 = String(h) + "1";
    let r1 = "";
    for (let m = 0; m < s1.length; m++) { r1 = s1[m] + r1; }
    const arr1 = [h % 3, h % 5, h % 7, 1];
    let acc1 = 0;
    for (const v of arr1) { acc1 += v * (v + 1); }
    const obj1: any = { k: 1, s: r1, n: acc1 };
    h = (h * 31 + obj1.n + obj1.s.length + (dyn["k1"] | 0)) % 1000003;
    if (typeof dyn["f1"] === "function") { h = (h + dyn["f1"](1)) % 1000003; }
  }
  // block 2
  {
    const s2 = String(h) + "2";
    let r2 = "";
    for (let m = 0; m < s2.length; m++) { r2 = s2[m] + r2; }
    const arr2 = [h % 3, h % 5, h % 7, 2];
    let acc2 = 0;
    for (const v of arr2) { acc2 += v * (v + 1); }
    const obj2: any = { k: 2, s: r2, n: acc2 };
    h = (h * 31 + obj2.n + obj2.s.length + (dyn["k2"] | 0)) % 1000003;
    if (typeof dyn["f2"] === "function") { h = (h + dyn["f2"](2)) % 1000003; }
  }
  // block 3
  {
    const s3 = String(h) + "3";
    let r3 = "";
    for (let m = 0; m < s3.length; m++) { r3 = s3[m] + r3; }
    const arr3 = [h % 3, h % 5, h % 7, 3];
    let acc3 = 0;
    for (const v of arr3) { acc3 += v * (v + 1); }
    const obj3: any = { k: 3, s: r3, n: acc3 };
    h = (h * 31 + obj3.n + obj3.s.length + (dyn["k3"] | 0)) % 1000003;
    if (typeof dyn["f3"] === "function") { h = (h + dyn["f3"](3)) % 1000003; }
  }
  // block 4
  {
    const s4 = String(h) + "4";
    let r4 = "";
    for (let m = 0; m < s4.length; m++) { r4 = s4[m] + r4; }
    const arr4 = [h % 3, h % 5, h % 7, 4];
    let acc4 = 0;
    for (const v of arr4) { acc4 += v * (v + 1); }
    const obj4: any = { k: 4, s: r4, n: acc4 };
    h = (h * 31 + obj4.n + obj4.s.length + (dyn["k4"] | 0)) % 1000003;
    if (typeof dyn["f4"] === "function") { h = (h + dyn["f4"](4)) % 1000003; }
  }
  // block 5
  {
    const s5 = String(h) + "5";
    let r5 = "";
    for (let m = 0; m < s5.length; m++) { r5 = s5[m] + r5; }
    const arr5 = [h % 3, h % 5, h % 7, 5];
    let acc5 = 0;
    for (const v of arr5) { acc5 += v * (v + 1); }
    const obj5: any = { k: 5, s: r5, n: acc5 };
    h = (h * 31 + obj5.n + obj5.s.length + (dyn["k5"] | 0)) % 1000003;
    if (typeof dyn["f5"] === "function") { h = (h + dyn["f5"](5)) % 1000003; }
  }
  // block 6
  {
    const s6 = String(h) + "6";
    let r6 = "";
    for (let m = 0; m < s6.length; m++) { r6 = s6[m] + r6; }
    const arr6 = [h % 3, h % 5, h % 7, 6];
    let acc6 = 0;
    for (const v of arr6) { acc6 += v * (v + 1); }
    const obj6: any = { k: 6, s: r6, n: acc6 };
    h = (h * 31 + obj6.n + obj6.s.length + (dyn["k6"] | 0)) % 1000003;
    if (typeof dyn["f6"] === "function") { h = (h + dyn["f6"](6)) % 1000003; }
  }
  // block 7
  {
    const s7 = String(h) + "7";
    let r7 = "";
    for (let m = 0; m < s7.length; m++) { r7 = s7[m] + r7; }
    const arr7 = [h % 3, h % 5, h % 7, 7];
    let acc7 = 0;
    for (const v of arr7) { acc7 += v * (v + 1); }
    const obj7: any = { k: 7, s: r7, n: acc7 };
    h = (h * 31 + obj7.n + obj7.s.length + (dyn["k7"] | 0)) % 1000003;
    if (typeof dyn["f7"] === "function") { h = (h + dyn["f7"](7)) % 1000003; }
  }
  // block 8
  {
    const s8 = String(h) + "8";
    let r8 = "";
    for (let m = 0; m < s8.length; m++) { r8 = s8[m] + r8; }
    const arr8 = [h % 3, h % 5, h % 7, 8];
    let acc8 = 0;
    for (const v of arr8) { acc8 += v * (v + 1); }
    const obj8: any = { k: 8, s: r8, n: acc8 };
    h = (h * 31 + obj8.n + obj8.s.length + (dyn["k8"] | 0)) % 1000003;
    if (typeof dyn["f8"] === "function") { h = (h + dyn["f8"](8)) % 1000003; }
  }
  // block 9
  {
    const s9 = String(h) + "9";
    let r9 = "";
    for (let m = 0; m < s9.length; m++) { r9 = s9[m] + r9; }
    const arr9 = [h % 3, h % 5, h % 7, 9];
    let acc9 = 0;
    for (const v of arr9) { acc9 += v * (v + 1); }
    const obj9: any = { k: 9, s: r9, n: acc9 };
    h = (h * 31 + obj9.n + obj9.s.length + (dyn["k9"] | 0)) % 1000003;
    if (typeof dyn["f9"] === "function") { h = (h + dyn["f9"](9)) % 1000003; }
  }
  // block 10
  {
    const s10 = String(h) + "10";
    let r10 = "";
    for (let m = 0; m < s10.length; m++) { r10 = s10[m] + r10; }
    const arr10 = [h % 3, h % 5, h % 7, 10];
    let acc10 = 0;
    for (const v of arr10) { acc10 += v * (v + 1); }
    const obj10: any = { k: 10, s: r10, n: acc10 };
    h = (h * 31 + obj10.n + obj10.s.length + (dyn["k10"] | 0)) % 1000003;
    if (typeof dyn["f10"] === "function") { h = (h + dyn["f10"](10)) % 1000003; }
  }
  // block 11
  {
    const s11 = String(h) + "11";
    let r11 = "";
    for (let m = 0; m < s11.length; m++) { r11 = s11[m] + r11; }
    const arr11 = [h % 3, h % 5, h % 7, 11];
    let acc11 = 0;
    for (const v of arr11) { acc11 += v * (v + 1); }
    const obj11: any = { k: 11, s: r11, n: acc11 };
    h = (h * 31 + obj11.n + obj11.s.length + (dyn["k11"] | 0)) % 1000003;
    if (typeof dyn["f11"] === "function") { h = (h + dyn["f11"](11)) % 1000003; }
  }
  // block 12
  {
    const s12 = String(h) + "12";
    let r12 = "";
    for (let m = 0; m < s12.length; m++) { r12 = s12[m] + r12; }
    const arr12 = [h % 3, h % 5, h % 7, 12];
    let acc12 = 0;
    for (const v of arr12) { acc12 += v * (v + 1); }
    const obj12: any = { k: 12, s: r12, n: acc12 };
    h = (h * 31 + obj12.n + obj12.s.length + (dyn["k12"] | 0)) % 1000003;
    if (typeof dyn["f12"] === "function") { h = (h + dyn["f12"](12)) % 1000003; }
  }
  // block 13
  {
    const s13 = String(h) + "13";
    let r13 = "";
    for (let m = 0; m < s13.length; m++) { r13 = s13[m] + r13; }
    const arr13 = [h % 3, h % 5, h % 7, 13];
    let acc13 = 0;
    for (const v of arr13) { acc13 += v * (v + 1); }
    const obj13: any = { k: 13, s: r13, n: acc13 };
    h = (h * 31 + obj13.n + obj13.s.length + (dyn["k13"] | 0)) % 1000003;
    if (typeof dyn["f13"] === "function") { h = (h + dyn["f13"](13)) % 1000003; }
  }
  // block 14
  {
    const s14 = String(h) + "14";
    let r14 = "";
    for (let m = 0; m < s14.length; m++) { r14 = s14[m] + r14; }
    const arr14 = [h % 3, h % 5, h % 7, 14];
    let acc14 = 0;
    for (const v of arr14) { acc14 += v * (v + 1); }
    const obj14: any = { k: 14, s: r14, n: acc14 };
    h = (h * 31 + obj14.n + obj14.s.length + (dyn["k14"] | 0)) % 1000003;
    if (typeof dyn["f14"] === "function") { h = (h + dyn["f14"](14)) % 1000003; }
  }
  // block 15
  {
    const s15 = String(h) + "15";
    let r15 = "";
    for (let m = 0; m < s15.length; m++) { r15 = s15[m] + r15; }
    const arr15 = [h % 3, h % 5, h % 7, 15];
    let acc15 = 0;
    for (const v of arr15) { acc15 += v * (v + 1); }
    const obj15: any = { k: 15, s: r15, n: acc15 };
    h = (h * 31 + obj15.n + obj15.s.length + (dyn["k15"] | 0)) % 1000003;
    if (typeof dyn["f15"] === "function") { h = (h + dyn["f15"](15)) % 1000003; }
  }
  // block 16
  {
    const s16 = String(h) + "16";
    let r16 = "";
    for (let m = 0; m < s16.length; m++) { r16 = s16[m] + r16; }
    const arr16 = [h % 3, h % 5, h % 7, 16];
    let acc16 = 0;
    for (const v of arr16) { acc16 += v * (v + 1); }
    const obj16: any = { k: 16, s: r16, n: acc16 };
    h = (h * 31 + obj16.n + obj16.s.length + (dyn["k16"] | 0)) % 1000003;
    if (typeof dyn["f16"] === "function") { h = (h + dyn["f16"](16)) % 1000003; }
  }
  // block 17
  {
    const s17 = String(h) + "17";
    let r17 = "";
    for (let m = 0; m < s17.length; m++) { r17 = s17[m] + r17; }
    const arr17 = [h % 3, h % 5, h % 7, 17];
    let acc17 = 0;
    for (const v of arr17) { acc17 += v * (v + 1); }
    const obj17: any = { k: 17, s: r17, n: acc17 };
    h = (h * 31 + obj17.n + obj17.s.length + (dyn["k17"] | 0)) % 1000003;
    if (typeof dyn["f17"] === "function") { h = (h + dyn["f17"](17)) % 1000003; }
  }
  // block 18
  {
    const s18 = String(h) + "18";
    let r18 = "";
    for (let m = 0; m < s18.length; m++) { r18 = s18[m] + r18; }
    const arr18 = [h % 3, h % 5, h % 7, 18];
    let acc18 = 0;
    for (const v of arr18) { acc18 += v * (v + 1); }
    const obj18: any = { k: 18, s: r18, n: acc18 };
    h = (h * 31 + obj18.n + obj18.s.length + (dyn["k18"] | 0)) % 1000003;
    if (typeof dyn["f18"] === "function") { h = (h + dyn["f18"](18)) % 1000003; }
  }
  // block 19
  {
    const s19 = String(h) + "19";
    let r19 = "";
    for (let m = 0; m < s19.length; m++) { r19 = s19[m] + r19; }
    const arr19 = [h % 3, h % 5, h % 7, 19];
    let acc19 = 0;
    for (const v of arr19) { acc19 += v * (v + 1); }
    const obj19: any = { k: 19, s: r19, n: acc19 };
    h = (h * 31 + obj19.n + obj19.s.length + (dyn["k19"] | 0)) % 1000003;
    if (typeof dyn["f19"] === "function") { h = (h + dyn["f19"](19)) % 1000003; }
  }
  // block 20
  {
    const s20 = String(h) + "20";
    let r20 = "";
    for (let m = 0; m < s20.length; m++) { r20 = s20[m] + r20; }
    const arr20 = [h % 3, h % 5, h % 7, 20];
    let acc20 = 0;
    for (const v of arr20) { acc20 += v * (v + 1); }
    const obj20: any = { k: 20, s: r20, n: acc20 };
    h = (h * 31 + obj20.n + obj20.s.length + (dyn["k20"] | 0)) % 1000003;
    if (typeof dyn["f20"] === "function") { h = (h + dyn["f20"](20)) % 1000003; }
  }
  // block 21
  {
    const s21 = String(h) + "21";
    let r21 = "";
    for (let m = 0; m < s21.length; m++) { r21 = s21[m] + r21; }
    const arr21 = [h % 3, h % 5, h % 7, 21];
    let acc21 = 0;
    for (const v of arr21) { acc21 += v * (v + 1); }
    const obj21: any = { k: 21, s: r21, n: acc21 };
    h = (h * 31 + obj21.n + obj21.s.length + (dyn["k21"] | 0)) % 1000003;
    if (typeof dyn["f21"] === "function") { h = (h + dyn["f21"](21)) % 1000003; }
  }
  // block 22
  {
    const s22 = String(h) + "22";
    let r22 = "";
    for (let m = 0; m < s22.length; m++) { r22 = s22[m] + r22; }
    const arr22 = [h % 3, h % 5, h % 7, 22];
    let acc22 = 0;
    for (const v of arr22) { acc22 += v * (v + 1); }
    const obj22: any = { k: 22, s: r22, n: acc22 };
    h = (h * 31 + obj22.n + obj22.s.length + (dyn["k22"] | 0)) % 1000003;
    if (typeof dyn["f22"] === "function") { h = (h + dyn["f22"](22)) % 1000003; }
  }
  // block 23
  {
    const s23 = String(h) + "23";
    let r23 = "";
    for (let m = 0; m < s23.length; m++) { r23 = s23[m] + r23; }
    const arr23 = [h % 3, h % 5, h % 7, 23];
    let acc23 = 0;
    for (const v of arr23) { acc23 += v * (v + 1); }
    const obj23: any = { k: 23, s: r23, n: acc23 };
    h = (h * 31 + obj23.n + obj23.s.length + (dyn["k23"] | 0)) % 1000003;
    if (typeof dyn["f23"] === "function") { h = (h + dyn["f23"](23)) % 1000003; }
  }
  // block 24
  {
    const s24 = String(h) + "24";
    let r24 = "";
    for (let m = 0; m < s24.length; m++) { r24 = s24[m] + r24; }
    const arr24 = [h % 3, h % 5, h % 7, 24];
    let acc24 = 0;
    for (const v of arr24) { acc24 += v * (v + 1); }
    const obj24: any = { k: 24, s: r24, n: acc24 };
    h = (h * 31 + obj24.n + obj24.s.length + (dyn["k24"] | 0)) % 1000003;
    if (typeof dyn["f24"] === "function") { h = (h + dyn["f24"](24)) % 1000003; }
  }
  // block 25
  {
    const s25 = String(h) + "25";
    let r25 = "";
    for (let m = 0; m < s25.length; m++) { r25 = s25[m] + r25; }
    const arr25 = [h % 3, h % 5, h % 7, 25];
    let acc25 = 0;
    for (const v of arr25) { acc25 += v * (v + 1); }
    const obj25: any = { k: 25, s: r25, n: acc25 };
    h = (h * 31 + obj25.n + obj25.s.length + (dyn["k25"] | 0)) % 1000003;
    if (typeof dyn["f25"] === "function") { h = (h + dyn["f25"](25)) % 1000003; }
  }
  // block 26
  {
    const s26 = String(h) + "26";
    let r26 = "";
    for (let m = 0; m < s26.length; m++) { r26 = s26[m] + r26; }
    const arr26 = [h % 3, h % 5, h % 7, 26];
    let acc26 = 0;
    for (const v of arr26) { acc26 += v * (v + 1); }
    const obj26: any = { k: 26, s: r26, n: acc26 };
    h = (h * 31 + obj26.n + obj26.s.length + (dyn["k26"] | 0)) % 1000003;
    if (typeof dyn["f26"] === "function") { h = (h + dyn["f26"](26)) % 1000003; }
  }
  // block 27
  {
    const s27 = String(h) + "27";
    let r27 = "";
    for (let m = 0; m < s27.length; m++) { r27 = s27[m] + r27; }
    const arr27 = [h % 3, h % 5, h % 7, 27];
    let acc27 = 0;
    for (const v of arr27) { acc27 += v * (v + 1); }
    const obj27: any = { k: 27, s: r27, n: acc27 };
    h = (h * 31 + obj27.n + obj27.s.length + (dyn["k27"] | 0)) % 1000003;
    if (typeof dyn["f27"] === "function") { h = (h + dyn["f27"](27)) % 1000003; }
  }
  // block 28
  {
    const s28 = String(h) + "28";
    let r28 = "";
    for (let m = 0; m < s28.length; m++) { r28 = s28[m] + r28; }
    const arr28 = [h % 3, h % 5, h % 7, 28];
    let acc28 = 0;
    for (const v of arr28) { acc28 += v * (v + 1); }
    const obj28: any = { k: 28, s: r28, n: acc28 };
    h = (h * 31 + obj28.n + obj28.s.length + (dyn["k28"] | 0)) % 1000003;
    if (typeof dyn["f28"] === "function") { h = (h + dyn["f28"](28)) % 1000003; }
  }
  // block 29
  {
    const s29 = String(h) + "29";
    let r29 = "";
    for (let m = 0; m < s29.length; m++) { r29 = s29[m] + r29; }
    const arr29 = [h % 3, h % 5, h % 7, 29];
    let acc29 = 0;
    for (const v of arr29) { acc29 += v * (v + 1); }
    const obj29: any = { k: 29, s: r29, n: acc29 };
    h = (h * 31 + obj29.n + obj29.s.length + (dyn["k29"] | 0)) % 1000003;
    if (typeof dyn["f29"] === "function") { h = (h + dyn["f29"](29)) % 1000003; }
  }
  // block 30
  {
    const s30 = String(h) + "30";
    let r30 = "";
    for (let m = 0; m < s30.length; m++) { r30 = s30[m] + r30; }
    const arr30 = [h % 3, h % 5, h % 7, 30];
    let acc30 = 0;
    for (const v of arr30) { acc30 += v * (v + 1); }
    const obj30: any = { k: 30, s: r30, n: acc30 };
    h = (h * 31 + obj30.n + obj30.s.length + (dyn["k30"] | 0)) % 1000003;
    if (typeof dyn["f30"] === "function") { h = (h + dyn["f30"](30)) % 1000003; }
  }
  // block 31
  {
    const s31 = String(h) + "31";
    let r31 = "";
    for (let m = 0; m < s31.length; m++) { r31 = s31[m] + r31; }
    const arr31 = [h % 3, h % 5, h % 7, 31];
    let acc31 = 0;
    for (const v of arr31) { acc31 += v * (v + 1); }
    const obj31: any = { k: 31, s: r31, n: acc31 };
    h = (h * 31 + obj31.n + obj31.s.length + (dyn["k31"] | 0)) % 1000003;
    if (typeof dyn["f31"] === "function") { h = (h + dyn["f31"](31)) % 1000003; }
  }
  // block 32
  {
    const s32 = String(h) + "32";
    let r32 = "";
    for (let m = 0; m < s32.length; m++) { r32 = s32[m] + r32; }
    const arr32 = [h % 3, h % 5, h % 7, 32];
    let acc32 = 0;
    for (const v of arr32) { acc32 += v * (v + 1); }
    const obj32: any = { k: 32, s: r32, n: acc32 };
    h = (h * 31 + obj32.n + obj32.s.length + (dyn["k32"] | 0)) % 1000003;
    if (typeof dyn["f32"] === "function") { h = (h + dyn["f32"](32)) % 1000003; }
  }
  // block 33
  {
    const s33 = String(h) + "33";
    let r33 = "";
    for (let m = 0; m < s33.length; m++) { r33 = s33[m] + r33; }
    const arr33 = [h % 3, h % 5, h % 7, 33];
    let acc33 = 0;
    for (const v of arr33) { acc33 += v * (v + 1); }
    const obj33: any = { k: 33, s: r33, n: acc33 };
    h = (h * 31 + obj33.n + obj33.s.length + (dyn["k33"] | 0)) % 1000003;
    if (typeof dyn["f33"] === "function") { h = (h + dyn["f33"](33)) % 1000003; }
  }
  // block 34
  {
    const s34 = String(h) + "34";
    let r34 = "";
    for (let m = 0; m < s34.length; m++) { r34 = s34[m] + r34; }
    const arr34 = [h % 3, h % 5, h % 7, 34];
    let acc34 = 0;
    for (const v of arr34) { acc34 += v * (v + 1); }
    const obj34: any = { k: 34, s: r34, n: acc34 };
    h = (h * 31 + obj34.n + obj34.s.length + (dyn["k34"] | 0)) % 1000003;
    if (typeof dyn["f34"] === "function") { h = (h + dyn["f34"](34)) % 1000003; }
  }
  // block 35
  {
    const s35 = String(h) + "35";
    let r35 = "";
    for (let m = 0; m < s35.length; m++) { r35 = s35[m] + r35; }
    const arr35 = [h % 3, h % 5, h % 7, 35];
    let acc35 = 0;
    for (const v of arr35) { acc35 += v * (v + 1); }
    const obj35: any = { k: 35, s: r35, n: acc35 };
    h = (h * 31 + obj35.n + obj35.s.length + (dyn["k35"] | 0)) % 1000003;
    if (typeof dyn["f35"] === "function") { h = (h + dyn["f35"](35)) % 1000003; }
  }
  // block 36
  {
    const s36 = String(h) + "36";
    let r36 = "";
    for (let m = 0; m < s36.length; m++) { r36 = s36[m] + r36; }
    const arr36 = [h % 3, h % 5, h % 7, 36];
    let acc36 = 0;
    for (const v of arr36) { acc36 += v * (v + 1); }
    const obj36: any = { k: 36, s: r36, n: acc36 };
    h = (h * 31 + obj36.n + obj36.s.length + (dyn["k36"] | 0)) % 1000003;
    if (typeof dyn["f36"] === "function") { h = (h + dyn["f36"](36)) % 1000003; }
  }
  // block 37
  {
    const s37 = String(h) + "37";
    let r37 = "";
    for (let m = 0; m < s37.length; m++) { r37 = s37[m] + r37; }
    const arr37 = [h % 3, h % 5, h % 7, 37];
    let acc37 = 0;
    for (const v of arr37) { acc37 += v * (v + 1); }
    const obj37: any = { k: 37, s: r37, n: acc37 };
    h = (h * 31 + obj37.n + obj37.s.length + (dyn["k37"] | 0)) % 1000003;
    if (typeof dyn["f37"] === "function") { h = (h + dyn["f37"](37)) % 1000003; }
  }
  // block 38
  {
    const s38 = String(h) + "38";
    let r38 = "";
    for (let m = 0; m < s38.length; m++) { r38 = s38[m] + r38; }
    const arr38 = [h % 3, h % 5, h % 7, 38];
    let acc38 = 0;
    for (const v of arr38) { acc38 += v * (v + 1); }
    const obj38: any = { k: 38, s: r38, n: acc38 };
    h = (h * 31 + obj38.n + obj38.s.length + (dyn["k38"] | 0)) % 1000003;
    if (typeof dyn["f38"] === "function") { h = (h + dyn["f38"](38)) % 1000003; }
  }
  // block 39
  {
    const s39 = String(h) + "39";
    let r39 = "";
    for (let m = 0; m < s39.length; m++) { r39 = s39[m] + r39; }
    const arr39 = [h % 3, h % 5, h % 7, 39];
    let acc39 = 0;
    for (const v of arr39) { acc39 += v * (v + 1); }
    const obj39: any = { k: 39, s: r39, n: acc39 };
    h = (h * 31 + obj39.n + obj39.s.length + (dyn["k39"] | 0)) % 1000003;
    if (typeof dyn["f39"] === "function") { h = (h + dyn["f39"](39)) % 1000003; }
  }
  out.push(h, la, lb, lc.la, lc.lb, late(1), keep(5), count);
  try {
    if (h >= 0) throw new Error("from the end " + (h % 13));
  } catch (e: any) {
    out.push(e.message);
  }
  return out;
}

const dyn: any = { bad: "{nope", k3: 3, f5: (v: number) => v * 9 };
console.log(JSON.stringify(big("hello split world", 12, [1, 2, 3], dyn)));
console.log(JSON.stringify(big(5, "y", null, { bad: "[1]" })));
function thrower(v: any): number {
  let h = 0;
  for (let i = 0; i < 3; i++) h += i;
  if (v === "boom") throw new TypeError("boom " + h);
  return h;
}
try { console.log(thrower(1)); thrower("boom"); } catch (e: any) { console.log("caught", e.name, e.message); }
