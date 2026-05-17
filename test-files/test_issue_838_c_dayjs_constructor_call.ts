// Issue #838 follow-up (c) — `this.<protoMethod>()` invoked inside a
// function-decl constructor body. dayjs's minified bundle does this:
// `function M(t){ this.$L = …; this.parse(t); this.$x = …; }`. The
// previous follow-up (b, #888) wired up the registration + new-construct
// halves so `typeof (new _).format === "function"` works, but the
// constructor body's own `this.parse(t)` call had to dispatch through
// the runtime tower while the instance was still being built (so
// `keys_array` either is null or only has the fields the constructor
// has already written). That dispatch reaches
// `js_native_call_method` with the synthetic class id stamped on the
// instance, and the fix path is the `lookup_prototype_method` walk
// after the field-scan and vtable-walk both miss.
//
// This test exercises three increments of constructor-body activity
// before the prototype call: (1) zero field writes (`keys_array` null
// at the call site), (2) one field write (small `keys_array`), and
// (3) a chained pattern where the prototype call mutates the instance
// and the constructor body then reads what was written. All three
// should land on the prototype method and produce the expected
// instance state.

// (1) Zero own-props at the time of the `this.parse()` call. Pre-fix
// the synthetic-class-id walk only fired after the vtable lookup, but
// for a synthetic id there's no vtable entry — so the walk fell
// through to "method is not a function". Post-fix the
// `lookup_prototype_method` arm sees the synthetic id and finds the
// closure.
function A(s: string) {
  (this as any).parse();
  (this as any).s = s;
}
(A as any).prototype.parse = function () {
  (this as any).flag = true;
};
const a = new (A as any)("hi");
console.log("a.flag:", (a as any).flag);
console.log("a.s:", (a as any).s);

// (2) One own-prop written before the `this.parse()` call — the case
// dayjs actually hits (`this.$L = w(t.locale, null, true)` comes
// first). The keys_array scan walks past the lone "s" entry and then
// the prototype-method walk picks up "parse".
function B(s: string) {
  (this as any).s = s;
  (this as any).parse();
}
(B as any).prototype.parse = function () {
  (this as any).parsed = ((this as any).s as string).toUpperCase();
};
const b = new (B as any)("hi");
console.log("b.parsed:", (b as any).parsed);

// (3) The prototype method writes a new field on `this`, and the
// constructor body reads it back after the call returns. This guards
// against a regression where the IMPLICIT_THIS save/restore around
// the prototype-method dispatch leaves the constructor's `this` slot
// pointing at the wrong receiver — the post-call read would then
// land on a stale instance.
function C(s: string) {
  (this as any).s = s;
  (this as any).parse();
  (this as any).suffix = "!" + (this as any).parsed;
}
(C as any).prototype.parse = function () {
  (this as any).parsed = ((this as any).s as string).toUpperCase();
};
const c = new (C as any)("hi");
console.log("c.parsed:", (c as any).parsed);
console.log("c.suffix:", (c as any).suffix);
