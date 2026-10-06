// RegExp spec-observability rows (REGEXP_ONE_SHAPE_DESIGN S0). Every row here
// must match Node through the one-shape slices (S1-S6). S3 also covers
// the formerly wrong flags getter observation.
//
// Rows observe: a patched or own `test`/`exec`, an overridden flag getter, a
// patched @@replace, subclassing, own/prototype property layout, `lastIndex`
// descriptor behaviour, literal identity, `compile`, structuredClone.
// One output line per row; a throw is printed as `throw <ErrorName>`.
function t(name: string, f: () => any): void {
  let line: string;
  try {
    line = name + ": " + JSON.stringify(f());
  } catch (e: any) {
    line = name + ": throw " + (e && e.constructor && e.constructor.name);
  }
  console.log(line);
}
const mk = (): any => /a/;

// This must precede every literal birth; a warmed birth shape masks the bug.
t("cold_intrinsic_after_global_reassignment", () => {
  const saved: any = globalThis.RegExp;
  const intrinsic: any = saved.prototype;
  globalThis.RegExp = function Fake() {} as any;
  try {
    const r: any = /a/;
    return [Object.getPrototypeOf(r) === intrinsic, r.test("a"), intrinsic.global, intrinsic.source, intrinsic.flags];
  } finally { globalThis.RegExp = saved; }
});
t("cold_lastIndex_define", () => {
  let calls = 0;
  Object.defineProperty(Object.prototype, "lastIndex", { set() { calls++; }, configurable: true });
  try { const r: any = /a/; return [calls, r.lastIndex, Object.getOwnPropertyNames(r)]; }
  finally { delete Object.prototype.lastIndex; }
});
t("proto_test_patched_untyped", () => { const o = RegExp.prototype.test; RegExp.prototype.test = function () { return "patched"; }; try { return mk().test("a"); } finally { RegExp.prototype.test = o; } });
t("proto_test_patched_literal", () => { const o = RegExp.prototype.test; RegExp.prototype.test = function () { return "patched"; }; try { return /a/.test("a"); } finally { RegExp.prototype.test = o; } });
t("own_exec_override", () => { const r: any = /a/; r.exec = function () { return null; }; return r.test("a"); });
t("proto_exec_override", () => { const o = RegExp.prototype.exec; RegExp.prototype.exec = function () { return null; }; try { return /a/.test("a"); } finally { RegExp.prototype.exec = o; } });
t("global_getter_override", () => { const d = Object.getOwnPropertyDescriptor(RegExp.prototype, "global"); Object.defineProperty(RegExp.prototype, "global", { get() { return "G"; }, configurable: true }); try { return mk().global; } finally { Object.defineProperty(RegExp.prototype, "global", d); } });
t("subclass_field", () => { class R extends RegExp { x = 1; } const r = new R("a", "g"); return [r.test("a"), r.lastIndex, r.x, Object.keys(r), r.source, r.global]; });
t("lastIndex_desc", () => Object.getOwnPropertyDescriptor(/a/, "lastIndex"));
t("own_names", () => Object.getOwnPropertyNames(/a/g));
t("lastIndex_nonwritable_test", () => { const r: any = /a/g; Object.defineProperty(r, "lastIndex", { writable: false }); return r.test("a"); });
t("lastIndex_roundtrip_obj", () => { const r: any = /a/; const o = {}; r.lastIndex = o; return r.lastIndex === o; });
t("own_test_shadow", () => { const r: any = mk(); r.test = () => "own"; return r.test("a"); });
t("symbol_replace_patched", () => { const o = RegExp.prototype[Symbol.replace]; RegExp.prototype[Symbol.replace] = function () { return "R"; }; try { return "abc".replace(/b/, "x"); } finally { RegExp.prototype[Symbol.replace] = o; } });
t("global_on_proto", () => RegExp.prototype.global);
t("global_getter_plain_this", () => Object.getOwnPropertyDescriptor(RegExp.prototype, "global").get.call({}));
t("literal_fresh", () => { const f = () => /a/g; const a = f(), b = f(); a.lastIndex = 3; return [a === b, b.lastIndex]; });
t("literal_state", () => { const f = () => /a/g; const r = f(); r.test("aa"); return [r.lastIndex, f().lastIndex]; });
t("compile", () => { const r: any = /a/; r.compile("b", "g"); return [r.source, r.global, r.test("b")]; });
t("structuredClone", () => { const r = structuredClone(/a/gi); return [r instanceof RegExp, r.flags, r.source]; });
t("in_operator", () => ["lastIndex" in /a/, "global" in /a/, Object.hasOwn(/a/, "global")]);

// S2: ordinary RegExp slots and prototype behavior.
t("proto_test_patched_local", () => { const r: any = /a/; const o = RegExp.prototype.test; RegExp.prototype.test = function () { return "patched"; }; try { return r.test("a"); } finally { RegExp.prototype.test = o; } });
t("subclass_exec", () => { class R extends RegExp { exec(s: any) { return null; } } const r = new R("a"); return [r.test("a"), r instanceof RegExp, r instanceof R, Object.prototype.toString.call(r)]; });
t("expando", () => { const r: any = /a/; r.foo = 1; return [r.foo, Object.keys(r), JSON.stringify(r)]; });
t("setPrototypeOf", () => { const r: any = mk(); Object.setPrototypeOf(r, { test() { return "p"; } }); return r.test("a"); });
t("delete_lastIndex", () => { "use strict"; const r: any = /a/; return delete r.lastIndex; });

// Intrinsic namespace, immutable data, constructor and clone regressions.
t("exec_brand", () => RegExp.prototype.exec.call({}, "a"));
t("private_spelling", () => {
  const r: any = /a/;
  r["#<perry:private-value:0:@0:[[RegExpMatcher]]>"] = 7;
  return [r.test("a"), r["#<perry:private-value:0:@0:[[RegExpMatcher]]>"], Object.getOwnPropertyNames(r)];
});
t("private_spelling_reflection", () => {
  const r: any = /a/;
  const key = "#<perry:private-value:0:@0:[[RegExpMatcher]]>";
  const before = Object.getOwnPropertyDescriptor(r, key);
  r[key] = 7;
  const descriptor = Object.getOwnPropertyDescriptor(r, key);
  const assigned = Object.assign({}, r), spread = { ...r };
  const names = Reflect.ownKeys(r);
  const text = JSON.stringify(r);
  const removed = delete r[key];
  return [before === undefined, descriptor.value, assigned[key], spread[key], names,
    text, removed, r.test("a"), Object.getOwnPropertyNames(r)];
});
t("compile_shared_cell", () => {
  const f = () => /a/g;
  const a: any = f(), b: any = f(); a.compile("b", "i"); a.lastIndex = 4;
  return [a.source,a.flags,a.lastIndex,b.source,b.flags,b.lastIndex,b.test("a")];
});
t("clone_original", () => {
  const r: any = new RegExp("a/b\n", "gi"); r.lastIndex = 7; r.tag = 42;
  const clone: any = structuredClone(r);
  return [clone.source,clone.flags,clone.lastIndex,Object.keys(clone)];
});
t("compile_nonwritable", () => {
  const r: any = /a/; Object.defineProperty(r, "lastIndex", { writable: false });
  try { r.compile("b", "g"); } catch (e: any) { return [e.constructor.name,r.source,r.flags,r.lastIndex]; }
});
t("subclass_private", () => {
  class R extends RegExp {
    #x = 42;
    value() { return this.#x; }
  }
  const r: any = new R("a", "g");
  return [r.value(),r.test("a"),r.lastIndex,r instanceof R,r instanceof RegExp,Object.getOwnPropertyNames(r)];
});

t("intrinsic_prototype_after_global_reassignment", () => {
  const original = RegExp;
  const prototype = RegExp.prototype;
  try { globalThis.RegExp = function Fake() {} as any; return Object.getPrototypeOf(/a/) === prototype; }
  finally { globalThis.RegExp = original; }
});

t("subclass_flags_nested_gets", () => {
  class R extends RegExp {}
  const r = new R("a", "g");
  const getter = Object.getOwnPropertyDescriptor(RegExp.prototype, "flags")!.get!;
  return [r.flags, getter.call(r), Reflect.get(RegExp.prototype, "flags", r), r.global, "a+a".replace(r, "-"), "a+a".match(r)];
});


// An ordinary RegExp may be a prototype, but its private matcher is never inherited.
t("regexp_as_function_prototype", () => {
  const prototype = /a/g;
  function F() {}
  F.prototype = prototype;
  const value = new (F as any)();
  let result = "accepted";
  try { RegExp.prototype.exec.call(value, "a"); } catch (e: any) { result = e.constructor.name; }
  return [Object.getPrototypeOf(value) === prototype, value instanceof RegExp,
          Object.getOwnPropertyNames(value), result];
});

t("dictionary_exec_shadow", () => {
  const r: any = /a/g;
  for (let i = 0; i < 1100; i++) r["key" + i] = i;
  const first = r.test("a");
  r.lastIndex = 0;
  let calls = 0;
  r.exec = () => { calls++; return null; };
  return [first, r.test("a"), calls];
});

// S3: flags must read overridden boolean getters.
t("flags_reads_getters", () => { const warm: any = /a/g; if (warm.flags !== "g") return "warmup failed"; const d = Object.getOwnPropertyDescriptor(RegExp.prototype, "global"); Object.defineProperty(RegExp.prototype, "global", { get() { return false; }, configurable: true }); try { return /a/g.flags; } finally { Object.defineProperty(RegExp.prototype, "global", d); } });
