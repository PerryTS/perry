// RegExp spec-observability rows (REGEXP_ONE_SHAPE_DESIGN S0). Every row here
// already matches node on main and must keep matching through the one-shape
// slices (S1-S6). The six rows that do not match yet live in
// test_gap_regexp_observe_known_wrong.ts.
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
