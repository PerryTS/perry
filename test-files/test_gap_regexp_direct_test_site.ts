// `re.test(s)` at a method site whose entry calls the builtin directly: the
// site's receiver and holder ShapeIds prove `test` and RegExpExec's
// `Get(R, "exec")` are the builtins (RegExp.prototype names both bodies as
// ConstFn lanes). Every row warms ONE site (`run`, whose arguments are plain
// parameters, so the lookup and the call are fused), then changes something
// the proof covers and calls through the same site again. Output must equal
// Node's. ORDER MATTERS: any store to RegExp.prototype.exec (even restoring
// the builtin) revokes its ConstFn lane for the rest of the run, so every row
// that needs the direct entry live comes before the first such store.
function run(re: any, s: any): any {
  return re.test(s);
}
// The argument is a call, so the lookup runs before it and the call after
// it (the split site): `exec` must be read when the call runs.
function runSplit(re: any, f: () => any): any {
  return re.test(f());
}
// Long enough to be a heap string (short strings are stored inline).
const HEAP = "a heap-allocated subject string";
function warm(re: any, s: any): void {
  for (let i = 0; i < 64; i++) run(re, s);
}
function t(name: string, f: () => any): void {
  let line: string;
  try {
    line = name + ": " + JSON.stringify(f());
  } catch (e: any) {
    line = name + ": throw " + (e && e.constructor && e.constructor.name);
  }
  console.log(line);
}

t("builtin", () => { const r = /a/; warm(r, "a"); return [run(r, "a"), run(r, "b")]; });
t("own_exec_after_warm", () => {
  const r: any = /a/; warm(r, "a");
  r.exec = function () { return { length: 1 }; };
  return [run(r, "b"), run(/a/, "b")];
});
t("expando_then_exec", () => {
  const r: any = /a/; warm(r, "a");
  r.foo = 1;
  const a = run(r, "a");
  r.exec = () => null;
  return [a, run(r, "a"), Object.keys(r)];
});
t("split_site_argument_adds_own_exec", () => {
  const r: any = /a/;
  for (let i = 0; i < 64; i++) runSplit(r, () => HEAP);
  return [runSplit(r, () => { r.exec = () => null; return HEAP; }), runSplit(r, () => HEAP)];
});
t("split_site_argument_patches_exec", () => {
  const r = /a/;
  for (let i = 0; i < 64; i++) runSplit(r, () => HEAP);
  const o = RegExp.prototype.exec;
  let calls = 0;
  try {
    return [runSplit(r, () => { RegExp.prototype.exec = function () { calls++; return null; }; return HEAP; }), calls];
  } finally { RegExp.prototype.exec = o; }
});
t("proto_exec_override_after_warm", () => {
  const r = /a/; warm(r, "a");
  const o = RegExp.prototype.exec;
  let calls = 0;
  RegExp.prototype.exec = function (this: any, s: any) { calls++; return null; };
  try { return [run(r, "a"), run(/a/, "a"), calls]; } finally { RegExp.prototype.exec = o; }
});
t("after_restore", () => { const r = /a/; warm(r, "a"); return [run(r, "a"), run(r, "b")]; });
t("proto_exec_same_function_rewritten", () => {
  const r = /a/; warm(r, "a");
  const o = RegExp.prototype.exec;
  RegExp.prototype.exec = o;
  return [run(r, "a"), run(r, "b")];
});
t("deleted_proto_exec", () => {
  const r = /a/; warm(r, "a");
  const d = Object.getOwnPropertyDescriptor(RegExp.prototype, "exec")!;
  delete (RegExp.prototype as any).exec;
  try { return [run(r, "a"), run(r, "b")]; } finally { Object.defineProperty(RegExp.prototype, "exec", d); }
});
t("proto_exec_getter", () => {
  const r = /a/; warm(r, "a");
  const d = Object.getOwnPropertyDescriptor(RegExp.prototype, "exec")!;
  let gets = 0;
  Object.defineProperty(RegExp.prototype, "exec", { get() { gets++; return d.value; }, configurable: true });
  try { return [run(r, "a"), run(r, "b"), gets]; } finally { Object.defineProperty(RegExp.prototype, "exec", d); }
});
t("proto_test_override_after_warm", () => {
  const r = /a/; warm(r, "a");
  const o = RegExp.prototype.test;
  RegExp.prototype.test = function () { return "patched"; } as any;
  try { return run(r, "a"); } finally { RegExp.prototype.test = o; }
});
t("subclass_exec", () => {
  class R extends RegExp { exec(s: string): any { return s === "x" ? [s] : null; } }
  const r = new R("a"); warm(/a/, "a");
  return [run(r, "x"), run(r, "a"), run(/a/, "a")];
});
t("subclass_plain", () => {
  class P extends RegExp {}
  const p = new P("a", "g"); warm(/a/, "a");
  return [run(p, "aa"), p.lastIndex, run(p, "aa"), p.lastIndex, run(p, "aa"), p.lastIndex];
});
t("setPrototypeOf", () => {
  const r = /a/; warm(r, "a");
  Object.setPrototypeOf(r, { test() { return "p"; } });
  return run(r, "a");
});
t("symbol_match_irrelevant", () => {
  const r: any = /a/; warm(r, "a");
  r[Symbol.match] = false;
  return [run(r, "a"), run(r, "b")];
});
t("global_lastIndex", () => {
  const r = /a/g; warm(/a/, "a");
  const out: any[] = [];
  for (let i = 0; i < 4; i++) out.push(run(r, "aa"), r.lastIndex);
  return out;
});
t("sticky_lastIndex", () => {
  const r = /a/y; warm(/a/, "a");
  const out: any[] = [];
  for (let i = 0; i < 3; i++) out.push(run(r, "aab"), r.lastIndex);
  r.lastIndex = 1; out.push(run(r, "ba"), r.lastIndex);
  return out;
});
t("global_lastIndex_past_end", () => { const r = /a/g; warm(/a/, "a"); r.lastIndex = 9; return [run(r, "aa"), r.lastIndex]; });
t("nonglobal_lastIndex_ignored", () => { const r = /a/; warm(r, "a"); r.lastIndex = 5; return [run(r, "a"), r.lastIndex]; });
t("nonglobal_lastIndex_valueOf_observed", () => {
  const r: any = /a/; warm(r, "a");
  let calls = 0;
  r.lastIndex = { valueOf() { calls++; return 3; } };
  return [run(r, "a"), calls, typeof r.lastIndex];
});
t("global_lastIndex_valueOf", () => {
  const r: any = /a/g; warm(/a/, "a");
  let calls = 0;
  r.lastIndex = { valueOf() { calls++; return 1; } };
  return [run(r, "aa"), calls, r.lastIndex];
});
t("lastIndex_nonwritable_global", () => {
  const r: any = /a/g; warm(/a/, "a");
  Object.defineProperty(r, "lastIndex", { writable: false });
  return run(r, "a");
});
t("lastIndex_nonwritable_nonglobal", () => {
  const r: any = /a/; warm(r, "a");
  Object.defineProperty(r, "lastIndex", { writable: false, value: 0 });
  return [run(r, "a"), run(r, "b")];
});
t("argument_coercion_order", () => {
  const r = /a/; warm(r, "a");
  const o = RegExp.prototype.exec;
  const log: string[] = [];
  const arg = { toString() { log.push("toString"); RegExp.prototype.exec = function () { log.push("exec"); return null; }; return "a"; } };
  try { return [run(r, arg), log]; } finally { RegExp.prototype.exec = o; }
});
t("argument_coercions", () => {
  const r = /^(undefined|null|1|true|\[object Object\])$/; warm(r, "1");
  return [run(r, undefined), run(r, null), run(r, 1), run(r, true), run(r, {}), run(r, Symbol.iterator === Symbol.iterator)];
});
t("receiver_not_regexp", () => {
  warm(/a/, "a");
  return [run({ test(s: string) { return "plain:" + s; } }, "a"), run(Object.create(RegExp.prototype), "a")];
});
t("compile_in_place", () => {
  const r: any = /a/; warm(r, "a");
  r.compile("b");
  return [run(r, "a"), run(r, "b"), r.source];
});
t("unicode_and_flags", () => {
  const r = /^\p{Default_Ignorable_Code_Point}$/u; warm(r, "​");
  const i = /A/i; warm(i, "a");
  return [run(r, "​"), run(r, "a"), run(i, "a"), run(i, "b")];
});
