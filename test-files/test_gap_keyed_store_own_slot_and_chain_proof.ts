// Keyed stores `o[k] = v` with run-time keys: own-slot overwrites answered by
// the receiver's shape or its overflow, and adds proved against the prototype
// chain, including the realm's Object.prototype before and after a program
// write to it. Module code is strict: a blocked store throws a TypeError.
const keys = ["alpha", "beta", "gamma", "delta"];
const P: any = { tag: "x" };
function attempt(label: string, f: () => void): void {
  try {
    f();
    console.log(label, "stored");
  } catch (e) {
    console.log(label, (e as any).constructor.name);
  }
}
let sum = 0;
for (let i = 0; i < 2000; i++) {
  const o: any = Object.create(P);
  o[keys[i & 3]] = i;
  o[keys[i & 3]] = i + 1;
  sum += o[keys[i & 3]] + (o.tag === "x" ? 1 : 0);
  const l: any = {};
  l[keys[(i + 1) & 3]] = i;
  sum += l[keys[(i + 1) & 3]];
}
console.log(sum);

// An inherited setter intercepts the keyed add, every time.
let setterCalls = 0;
const S: any = {};
Object.defineProperty(S, "beta", { set(v) { setterCalls += v; }, get() { return "g"; } });
for (let i = 0; i < 10; i++) { const o: any = Object.create(S); o[keys[1]] = 1; if (Object.keys(o).length !== 0) console.log("own key added"); }
console.log("setter", setterCalls);

// An inherited non-writable property blocks the add.
const R: any = Object.defineProperty({}, "gamma", { value: 5, writable: false });
const r: any = Object.create(R);
attempt("inherited readonly", () => { r[keys[2]] = 9; });
console.log("readonly", r.gamma, Object.keys(r).length);

// An own non-writable or accessor slot is not a plain overwrite.
const own: any = { alpha: 1, beta: 2 };
Object.defineProperty(own, "alpha", { writable: false });
attempt("own readonly", () => { own[keys[0]] = 7; });
let ownSet = 0;
Object.defineProperty(own, "beta", { set(v) { ownSet = v; }, get() { return 0; }, configurable: true });
own[keys[1]] = 8;
console.log("own", own.alpha, ownSet);

// Frozen and sealed receivers.
const fz: any = Object.freeze({ alpha: 1 });
attempt("frozen overwrite", () => { fz[keys[0]] = 2; });
attempt("frozen add", () => { fz[keys[1]] = 3; });
const sl: any = Object.seal({ alpha: 1 });
attempt("sealed overwrite", () => { sl[keys[0]] = 2; });
attempt("sealed add", () => { sl[keys[1]] = 3; });
console.log("frozen", fz.alpha, fz.beta, "sealed", sl.alpha, sl.beta);

// `__proto__` as a run-time key runs the Annex-B setter.
const pk = ["__proto__"];
const q: any = {};
q[pk[0]] = P;
console.log("proto", Object.getPrototypeOf(q) === P, Object.keys(q).length);

// Overflowed receivers (run-time keys on {} and Object.create(null)): an own
// entry is overwritten in place; readonly, accessor, frozen and sealed
// entries are not plain overwrites.
const ov: any = {};
for (let i = 0; i < 4; i++) ov[keys[i]] = i;
for (let i = 0; i < 400; i++) ov[keys[i & 3]] += 1;
console.log("overflow", ov.alpha, ov.beta, ov.gamma, ov.delta, Object.keys(ov).join());
const ovn: any = Object.create(null);
for (let i = 0; i < 4; i++) ovn[keys[i]] = i;
for (let i = 0; i < 40; i++) ovn[keys[i & 3]] = i;
console.log("overflow null", ovn.alpha, ovn.delta, Object.keys(ovn).join());
Object.defineProperty(ov, "alpha", { writable: false });
attempt("overflow readonly", () => { ov[keys[0]] = -1; });
let ovSet = 0;
Object.defineProperty(ov, "beta", { set(v) { ovSet = v; }, get() { return 0; }, configurable: true });
ov[keys[1]] = 42;
console.log("overflow attrs", ov.alpha, ovSet, ov.beta);
const ofz: any = {};
ofz[keys[2]] = 1;
Object.freeze(ofz);
attempt("overflow frozen", () => { ofz[keys[2]] = 2; });
const osl: any = {};
osl[keys[3]] = 1;
Object.seal(osl);
attempt("overflow sealed overwrite", () => { osl[keys[3]] = 2; });
attempt("overflow sealed add", () => { osl[keys[0]] = 3; });
console.log("overflow frozen", ofz.gamma, "sealed", osl.delta, osl.alpha);
// Many keys (the hashed index), long computed keys, an overflowed prototype.
const big: any = {};
for (let i = 0; i < 40; i++) big["key_number_" + i] = i;
for (let i = 0; i < 40; i++) big["key_number_" + i] = big["key_number_" + i] * 2;
const child: any = Object.create(big);
child["key_number_3"] = -3;
console.log("big", big.key_number_0, big.key_number_39, big.key_number_3, child.key_number_17, child.key_number_3);

// A program write to Object.prototype: a setter there now intercepts adds on
// plain and Object.create objects alike.
let protoSetter = 0;
Object.defineProperty(Object.prototype, "delta", { set(v) { protoSetter += v; }, get() { return "inherited"; }, configurable: true });
for (let i = 0; i < 5; i++) { const a: any = {}; a[keys[3]] = 1; const b: any = Object.create(P); b[keys[3]] = 2; if (Object.keys(a).length || Object.keys(b).length) console.log("added past setter"); }
console.log("proto setter", protoSetter);
delete (Object.prototype as any).delta;
const after: any = Object.create(P);
after[keys[3]] = 4;
console.log("after delete", after.delta, Object.keys(after));
