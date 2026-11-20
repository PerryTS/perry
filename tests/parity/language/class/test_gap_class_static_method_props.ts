// static methods are own data properties of the class function object
class P {
  static a() { return "P.a:" + (this === P ? "P" : (this as any).name); }
  static get g() { return 1; }
  static b(x: number, y: number) { return x + y; }
  static z = 3;
}
class Q extends P {}
const d = Object.getOwnPropertyDescriptor(P, "a")!;
console.log(typeof d.value, d.writable, d.enumerable, d.configurable);
console.log(P.a === P.a, Q.a === P.a, d.value === P.a);
console.log(Object.prototype.hasOwnProperty.call(Q, "a"), Object.keys(P).join(","));
console.log(Object.getOwnPropertyNames(P).join(","));
console.log(P.a(), Q.a(), P.b.length, P.b.name, P.b(2, 3));
const saved = P.a;
console.log(saved.call(Q));
(P as any).a = function () { return "replaced"; };
console.log(P.a(), Q.a(), saved.call(P));
delete (P as any).a;
console.log(typeof (P as any).a, typeof (Q as any).a, "a" in P);
for (const k in P) console.log("enum", k);
console.log(Object.isFrozen(Object.freeze(Q)), Object.getOwnPropertyDescriptor(P, "b")!.configurable);
// One call site, armed before the store: its memo must see the store.
class R {
  static m() { return "R.m"; }
}
class S extends R {}
function callBoth() { return R.m() + "," + S.m(); }
function callValue(c: any) { return c.m(); }
for (let i = 0; i < 3; i++) console.log(callBoth(), callValue(R), callValue(S));
(R as any).m = function () { return "stored:" + (this === S ? "S" : "R"); };
console.log(callBoth(), callValue(R), callValue(S));
(S as any).m = function () { return "own S"; };
console.log(callBoth(), callValue(S));
delete (R as any).m;
delete (S as any).m;
console.log(typeof (R as any).m, typeof (S as any).m);
// A class nothing inherits from: its shape changes only because the
// declaration was replaced, redefined or deleted.
class T {
  static m() { return "T.m"; }
}
function callT() { return T.m(); }
function callTv(c: any) { return c.m(); }
for (let i = 0; i < 3; i++) console.log(callT(), callTv(T));
(T as any).m = function () { return "T stored"; };
console.log(callT(), callTv(T));
Object.defineProperty(T, "m", { value: function () { return "T defined"; } });
console.log(callT(), callTv(T));
delete (T as any).m;
try { callT(); } catch (e) { console.log("callT", e instanceof TypeError); }
try { callTv(T); } catch (e) { console.log("callTv", e instanceof TypeError); }
// Every store form over an armed site's declaration reaches the call.
const forms: [string, (R: any) => void][] = [
  ["assign", (R) => { R.m = function () { return "assign"; }; }],
  ["define", (R) => { Object.defineProperty(R, "m", { value: function () { return "define"; } }); }],
  ["reflect", (R) => { Reflect.set(R, "m", function () { return "reflect"; }); }],
  ["assignObj", (R) => { Object.assign(R, { m: function () { return "assignObj"; } }); }],
  ["defineAll", (R) => { Object.defineProperties(R, { m: { value: function () { return "defineAll"; } } }); }],
];
class A1 { static m() { return "decl"; } }
function s1() { return A1.m(); }
class A2 { static m() { return "decl"; } }
function s2() { return A2.m(); }
class A3 { static m() { return "decl"; } }
function s3() { return A3.m(); }
class A4 { static m() { return "decl"; } }
function s4() { return A4.m(); }
class A5 { static m() { return "decl"; } }
function s5() { return A5.m(); }
const cs: any[] = [A1, A2, A3, A4, A5];
const ss = [s1, s2, s3, s4, s5];
for (let i = 0; i < 5; i++) {
  ss[i](); ss[i]();
  forms[i][1](cs[i]);
  console.log(forms[i][0], ss[i]());
}
