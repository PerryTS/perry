use super::*;

#[test]
fn assignment_pattern_defaults_apply_to_undefined() {
    assert_eq!(
        run_web_source(
            r#"
let x: any, y: any, p: any;
[x = 5] = [];
console.log(x);
[x = 5, y = 6] = [undefined, null];
console.log(x, y);
[x = 5] = [0];
console.log(x);
({ a: p = 8 } = {} as any);
console.log(p);
[[x = 1], { b: y = 2 }] = [[], {}] as any;
console.log(x, y);
for ([x = 10] of [[], [7]] as any[]) console.log(x);
"#
        ),
        "5\n5 null\n0\n8\n1 2\n10\n7"
    );
}

/// #14734: a bridge call stores its arguments only into the frame it opened.
/// A slot above the open frame is where the next nested frame is written, so
/// the calls overwrote each other's arguments (`join` on an `any` receiver, and
/// the `class_call_method` fallback that a value-position `o.list.push(v)` takes).
#[test]
fn bridge_calls_store_only_into_the_frame_they_open() {
    assert_eq!(
        run_web_source(
            r#"
function f(s: any) { return s.join(","); }
function g(s: any, t: any) { return s.join("-") + "|" + t.join("+"); }
console.log(f([1, 2]));
console.log("pair", g([1, 2], [3, 4]), g(["a"], ["b", "c"]));
const o = { list: [] as number[] };
console.log("push", o.list.push(7), o.list.push(8), o.list.join(","));
"#
        ),
        "1,2\npair 1-2|3+4 a|b+c\npush 1 2 7,8"
    );
}

// A tagged template calls its tag with the call site's strings array, whose `raw`
// holds the unescaped text and which is the same object on every evaluation, through
// a static method, an instance method or a function.
#[test]
fn a_tagged_template_passes_its_strings_array_to_any_tag() {
    assert_eq!(
        run_web_source(
            r#"
class T { static tag(s: string[], a: any, b: any) { return s.join("|") + "/" + a + "," + b; } }
console.log(T.tag`a${1}b${2}c`);
class K { m = "M"; tag(s: any, v: any) { return this.m + s[0] + v + s[1]; } }
console.log(new K().tag`v=${3}!`);
function raw(s: TemplateStringsArray) { return s.raw[0] + "|" + s.raw.length + "|" + s[0]; }
console.log(raw`a\nb`);
function anyRaw(s: any) { return s.raw[0] + "," + s.raw[1]; }
console.log(anyRaw`x\ty${0}z`);
function id(s: any) { return s; }
function site() { return id`q`; }
const a = site();
console.log(a === site(), a === id`q`, Array.isArray(a), a.length);
"#
        ),
        "a|b|c/1,2\nMv=3!\na\\nb|1|a\nb\nx\\ty,z\ntrue false true 1"
    );
}

/// #14733: every object pattern opens with RequireObjectCoercible on its
/// source, which hands the source through or throws a TypeError for a nullish one.
#[test]
fn object_patterns_bind_from_their_source_and_reject_nullish() {
    assert_eq!(
        run_web_source(
            r#"
const arr = [{ k: 1 }, { k: 2 }];
for (const { k } of arr) console.log("k", k);
const v: any = { a: 3, b: { c: 4 } };
const { a, b: { c } } = v;
console.log("decl", a, c);
let reads = 0;
function src(): any { reads++; return { x: 5 }; }
const { x } = src();
console.log("once", x, reads);
function nullish(o: any): string {
  try { const { y } = o; return "no throw " + y; } catch (e) { return "caught " + (e instanceof TypeError); }
}
console.log(nullish(null), nullish(undefined), nullish({ y: 6 }));
"#
        ),
        "k 1\nk 2\ndecl 3 4\nonce 5 1\ncaught true caught true no throw 6"
    );
}

// An object rest element is a new object holding the source's other own enumerable keys,
// in a synchronous and in an async body.
#[test]
fn object_rest_copies_the_keys_not_named_before_it() {
    assert_eq!(
        run_web_source(
            r#"
const keys = (o: any) => Object.keys(o).join(",");
function decl() { const { x, ...o } = { x: 1, y: 2, z: 3 } as any; return x + " " + keys(o); }
function param({ a, ...r }: any) { return a + " " + keys(r); }
function assign() { let x: any, o: any; ({ x, ...o } = { x: 1, y: 2 } as any); return x + " " + keys(o); }
function nested() { const { a: { b, ...inner }, ...outer } = { a: { b: 1, c: 2 }, d: 3 } as any; return b + " " + keys(inner) + " " + keys(outer); }
function copy() { const src: any = { p: 1, q: 2 }; const { ...o } = src; o.p = 9; return (o !== src) + " " + src.p + " " + keys(o); }
function computed() { const k = "a"; const { [k]: v, ...r } = { a: 1, b: 2 } as any; return v + " " + keys(r); }
function hidden() { const src: any = { a: 1, b: 2 }; Object.defineProperty(src, "h", { value: 3, enumerable: false }); const { a, ...r } = src; return keys(r) + " " + ("h" in r); }
function proto() { const { b, ...r } = JSON.parse('{"__proto__":{"f":1},"b":2}'); return Object.keys(r).join(",") + " " + r.f; }
async function inAsync() { const k = "z"; const { x, [k]: z, ...o } = { x: 1, y: 2, z: 3 } as any; return x + " " + z + " " + JSON.stringify(o); }
console.log(decl() + " | " + param({ a: 1, b: 2, c: 3 }) + " | " + assign() + " | " + nested());
console.log(copy() + " | " + computed() + " | " + hidden() + " | " + proto());
inAsync().then((v) => console.log(v));
"#
        ),
        "1 y,z | 1 b,c | 1 y | 1 c d\ntrue 1 p,q | 1 b | b false | __proto__ undefined\n1 3 {\"y\":2}"
    );
}

// In an async body `delete` removes the property, throws on a nullish receiver, and
// still evaluates an operand that is not a property reference.
#[test]
fn async_delete_reaches_the_property_reference() {
    assert_eq!(
        run_web_source(
            r#"
let n = 0;
function bump() { n++; return n; }
async function del() {
  const o: any = { a: 1, b: 2 }; const k = "b";
  const d = delete o.a; delete o[k];
  let caught = "no";
  try { const z: any = null; delete z.x; } catch (e) { caught = "yes"; }
  delete (bump() as any);
  return d + " " + JSON.stringify(o) + " " + caught + " " + n;
}
del().then((v) => console.log(v));
"#
        ),
        "true {} yes 1"
    );
}

// `.message` is an ordinary property read: only an Error's message comes from the
// Error itself.
#[test]
fn message_reads_the_property_of_any_object() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = JSON.parse('{"message":"hi","phone":"p"}');
const lit: any = { message: "lit" };
class G { get message(): string { return "getter"; } }
class F { message: string; constructor() { this.message = "field"; } }
const none: any = {};
console.log(o.message + " " + o.phone + " " + lit.message + " " + new G().message + " " + new F().message + " " + none.message);
try { throw { message: "thrown" }; } catch (e: any) { console.log(e.message); }
const err = new Error("boom");
const empty = new Error();
console.log(err.message + " [" + empty.message + "]");
try { JSON.parse("{bad"); } catch (e: any) { console.log(e.message.length > 0); }
"#
        ),
        "hi p lit getter field undefined\nthrown\nboom []\ntrue"
    );
}

#[test]
fn an_error_subclass_takes_its_message_from_super() {
    assert_eq!(
        run_web_source(
            r#"
class MyErr extends Error { constructor(m?: string) { super(m); this.name = "MyErr"; } }
class Coded extends Error { code: number; constructor(m: string, c: number) { super(m); this.code = c; } }
class Bare extends Error {}
class Deep extends Bare { constructor() { super("deep"); } }
class Ranged extends RangeError { constructor(m: string) { super(m); } }
class Fielded extends Error { code = 5; }
const c = new Coded("coded", 7);
console.log(new MyErr("sub").message + " [" + new MyErr().message + "] " + c.message + " " + c.code);
console.log(new Bare("bare").message + " " + new Deep().message + " " + new Ranged("ranged").message + " [" + new Fielded().message + "]");
try { throw new MyErr("caught"); } catch (e: any) { console.log(e.message + " " + (e instanceof Error)); }
console.log(JSON.stringify(new MyErr("hidden")) + " " + Object.keys(new Bare("k")).length);
"#
        ),
        "sub [] coded 7\nbare deep ranged []\ncaught true\n{\"name\":\"MyErr\"} 0"
    );
}

#[test]
fn a_thrown_builtin_error_subclass_keeps_its_name() {
    assert_eq!(
        run_web_source(
            r#"
try { throw new RangeError("x"); } catch (e: any) { console.log(e.name, e instanceof RangeError, e.message); }
const all = [new TypeError("t"), new ReferenceError("r"), new SyntaxError("s"), new RangeError("o", { cause: 1 }), new Error("e")];
console.log(all.map((e) => e.name + ":" + e.message).join(" "));
"#
        ),
        "RangeError true x\nTypeError:t ReferenceError:r SyntaxError:s RangeError:o Error:e"
    );
}

// `textSetFontWeight(widget, size, weight)` sets both; the weight is the native
// 0..1 scale, mapped onto CSS 100..900 and clamped.
#[test]
fn text_set_font_weight_applies_the_size_and_the_weight() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Text, textSetFontWeight } from "perry/ui";
const a = Text("A");
textSetFontWeight(a, 28, 1.0);
const b = Text("B");
textSetFontWeight(b, 16, 0.5);
const c = Text("C");
textSetFontWeight(c, 12, 700);
"#,
            r#"
for (const label of ["A", "B", "C"]) {
  const el = [...uiHandles.values()].find(e => e.textContent === label);
  console.log(label + " " + el.style.fontSize + " " + el.style.fontWeight);
}
"#
        ),
        "A 28px 900\nB 16px 500\nC 12px 900"
    );
}

// A class used as a value is a constructor: `new` on it runs the class's constructor
// and field initializers, and the instance keeps its methods and its class.
#[test]
fn a_class_used_as_a_value_constructs_its_instances() {
    assert_eq!(
        run_web_source(
            r#"
class K { v = 3; constructor(x?: number) { if (x !== undefined) this.v = x; } twice() { return this.v * 2; } }
class Bare { w = 4; }
const KK: any = K;
const kk = new KK(7);
console.log(typeof KK + " " + kk.v + " " + kk.twice() + " " + (kk instanceof K) + " " + new KK().v);
const r = { K, Bare };
console.log(typeof r.K + " " + new r.K(9).twice() + " " + new r.Bare().w + " " + (KK === r.K));
"#
        ),
        "function 7 14 true 3\nfunction 18 4 true"
    );
}

// `fetchWithAuth` / `fetchPostWithAuth` build their headers object between
// evaluating their arguments; the bridge must receive each argument in its own
// slot, evaluated in source order, and the module must validate at all.
#[test]
fn fetch_with_auth_passes_url_method_body_and_authorization() {
    assert_eq!(
        run_web_source_with_dom(
            r#"
function base(): string { return "http://h/" + String(6 * 7); }
function tok(): string { return "Bearer " + base().length; }
function arg(label: string, v: string): string { console.log(label); return v; }
fetchWithAuth(arg("url", base() + "/g"), arg("auth", `${tok()}-g`))
  .then(() => fetchPostWithAuth(arg("url", base() + "/p"), arg("auth", tok()), arg("body", JSON.stringify({ n: tok() }))))
  .then(() => console.log("done"));
"#,
            &format!(
                "{STUB_DOM}{}",
                r#"globalThis.fetch = (url, opts) => {
  console.log(JSON.stringify([url, opts]));
  return Promise.resolve({ text: () => Promise.resolve("ok") });
};"#
            ),
            ""
        ),
        concat!(
            "url\nauth\n",
            r#"["http://h/42/g",{"headers":{"Authorization":"Bearer 11-g"}}]"#,
            "\nurl\nauth\nbody\n",
            r#"["http://h/42/p",{"method":"POST","body":"{\"n\":\"Bearer 11\"}","headers":{"Authorization":"Bearer 11"}}]"#,
            "\ndone"
        )
    );
}

#[test]
fn property_update_stores_and_returns_the_old_or_new_value() {
    assert_eq!(
        run_web_source(
            "class P { x: number; constructor(x: number) { this.x = x } }
             const r = new P(5); const a = r.x++; console.log(a, r.x);
             const o: any = { x: 5 };
             const b = o.x++; console.log(b, o.x);
             const d = ++o.x; console.log(d, o.x);
             o.x--; console.log(o.x);
             const e = --o.x; console.log(e, o.x);
             o.x++; console.log(o.x);"
        ),
        "5 6\n5 6\n7 7\n6\n5 5\n6"
    );
}

// `for...in` visits the enumerable string keys of an object and its prototype chain:
// a literal, an array, a class instance, a boxed string, a key deleted before its
// turn is skipped, a closure and a method value have none. Expected output is Node's.
#[test]
fn for_in_visits_enumerable_keys_of_the_object_and_its_prototypes() {
    assert_eq!(
        run_web_source(
            r#"
const keysIn = (o: any) => { const ks: string[] = []; for (const k in o) ks.push(k); return ks.join(","); };
class P { x = 1; y = 2; m() { return 0; } }
function typed() { const o = { a: 1, b: 2 }; const ks: string[] = []; for (const k in o) ks.push(k); return ks.join(","); }
function inherited() { const o: any = { b: 2 }; Object.setPrototypeOf(o, { a: 1 }); Object.defineProperty(o, "h", { value: 3, enumerable: false }); return keysIn(o); }
function deleted() { const o: any = { a: 1, b: 2, c: 3 }; const ks: string[] = []; for (const k in o) { ks.push(k); delete o.b; } return ks.join(","); }
function letHead() { const o: any = { p: 1, q: 2 }; let k: string; let s = ""; for (k in o) s = s + k + o[k]; return s; }
console.log(typed() + " | " + keysIn({ u: 1, v: 2 }) + " | " + keysIn([10, 20]) + " | " + keysIn(new P()) + " | " + keysIn("ab"));
console.log(inherited() + " | " + deleted() + " | " + letHead() + " | [" + keysIn(null) + keysIn(undefined) + keysIn(5) + "]");
const g: any = () => 0; g.w = 1;
console.log("[" + keysIn(() => 1) + "] [" + keysIn(new P().m) + "] [" + keysIn(g) + "]");
"#
        ),
        "a,b | u,v | 0,1 | x,y | 0,1\nb,a | a,c | p1q2 | []\n[] [] [w]"
    );
}

#[test]
fn member_update_on_this_computed_keys_and_elements() {
    assert_eq!(
        run_web_source(
            "class C {
               n = 0; items: number[] = [0, 0];
               inc(): number { return this.n++; }
               pre(): number { return ++this.n; }
               bump(): number { this.n++; return this.items[1]++; }
             }
             const c = new C();
             console.log(c.inc(), c.n); console.log(c.pre(), c.n);
             console.log(c.bump(), c.n);
             console.log(c.items.join(','));
             const o: any = { y: 10 }; const k = 'y';
             const a = o[k]++; console.log(a, o.y);
             const b = ++o[k]; console.log(b, o.y);"
        ),
        "0 1\n2 2\n0 3\n0,1\n10 11\n12 12"
    );
}

#[test]
fn member_update_evaluates_base_and_key_once() {
    assert_eq!(
        run_web_source(
            "let calls = 0; const o: any = { x: 1 };
             function get(): any { calls++; return o; }
             const a = get().x++; console.log(a, o.x, calls);
             const b = ++get().x; console.log(b, o.x, calls);
             const arr = [5, 6]; let i = 0;
             const c = arr[i++]++; console.log(c, arr.join(','), i);"
        ),
        "1 2 1\n3 3 2\n5 6,6 1"
    );
}

#[test]
fn member_update_converts_the_old_value_to_a_number() {
    assert_eq!(
        run_web_source(
            "const o: any = { x: null, b: true, s: '5' };
             const a = o.x++; console.log(a, o.x);
             const b = o.b++; console.log(b, o.b);
             const c = ++o.s; console.log(c, o.s);
             const arr: any[] = [null]; const d = arr[0]--; console.log(d, arr[0]);"
        ),
        "0 1\n1 2\n6 6\n0 -1"
    );
}

// An async body is emitted as JS, which must lower `Object.keys/values/entries` itself
// rather than read them as undefined (#14633).
#[test]
fn async_body_lists_an_objects_keys_values_and_entries() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  const o: any = { a: 1, b: 2 };
  return Object.keys(o).join("|") + " " + Object.values(o).join("|") + " " + Object.entries(o).join("|");
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "a|b 1|2 a,1|b,2"
    );
}

// An async body is emitted as JS, which must build an array literal with a spread, call
// `Math.max` / `Math.min` and an array's `keys` / `values` / `entries` itself rather than
// yield undefined (#15100).
#[test]
fn async_body_spreads_into_array_literals_and_calls_math_max_min() {
    assert_eq!(
        run_web_source(
            r#"
let log = "";
function at(n: any) { log += n; return n; }
async function f(): Promise<string> {
  const xs = [1, 2];
  const ys: any[] = [0, ...xs, 3, , ];
  const ks = [...xs.keys(), ...xs.values(), ..."ab", [...xs.entries()][1][0]];
  const m = Math.max(at(3), at(9), at(4)) + "," + Math.min(at(3), at(9)) + "," + Math.max("7" as any, 2) + "," + Math.min(1, NaN) + "," + Math.max() + "," + Math.min() + "," + Math.min(Infinity, 5) + "," + Math.max(-Infinity, -1);
  return ys.length + " " + ys.join("|") + " " + ks.join(",") + " " + m + " " + log;
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "5 0|1|2|3| 0,1,1,2,a,b,1 9,3,7,NaN,-Infinity,Infinity,5,-1 39439"
    );
}

// `new Set([...])`, `new Map([...])` and their methods inside an async body build and use
// real collections (#14595).
#[test]
fn async_body_builds_and_uses_maps_and_sets() {
    assert_eq!(
        run_web_source(
            r#"
const outer = new Set([1, 2, 3]);
async function sets(): Promise<string> {
  const s = new Set([1, 2, 3]);
  const e = new Set<string>();
  e.add("a");
  e.add("a");
  const d1 = s.delete(1);
  const d2 = s.delete(1);
  e.add(await Promise.resolve("b"));
  const copy = new Set(s);
  return [s.size, s.has(2), s.has(1), e.size, d1, d2, outer.has(3), typeof s, copy.size, copy.has(3)].join(" ");
}
async function maps(): Promise<string> {
  const m = new Map<string, number>([["a", 1]]);
  const n = new Map<string, number>();
  m.set("b", 2).set("c", 3);
  n.set("x", 1).set("y", await Promise.resolve(4));
  const d1 = m.delete("a");
  const d2 = m.delete("zz");
  const before = [m.size, m.get("b"), m.get("a"), m.has("c"), m.has("a"), d1, d2, n.get("x"), n.get("y")].join(" ");
  m.clear();
  return before + " " + m.size;
}
// A typed parameter's methods lower to the dedicated Map/Set nodes.
async function typed(m: Map<string, number>, s: Set<number>): Promise<string> {
  m.set("q", 5).set("r", 6);
  const got = [m.get("q"), m.has("q"), m.delete("q"), m.delete("q"), m.size];
  m.clear();
  s.clear();
  return got.join(" ") + " " + m.size + " " + s.size;
}
sets().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
maps().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
typed(new Map(), new Set([1])).then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "5 true true false 1 0 0\n2 true false 2 true false true object 2 true\n2 2  true false true false 1 4 0"
    );
}

// `for...of` over an array and `i++` / `k--` in an async body: the array check of the
// for-of lowering and the update expression both reach the JS emitter (#14595).
#[test]
fn async_body_runs_for_of_over_an_array_and_update_expressions() {
    assert_eq!(
        run_web_source(
            r#"
async function f(xs: string[]): Promise<string> {
  let s = "";
  for (const p of xs) s = s + p;
  let n = 0;
  // The `n < 100` bound ends the loop even where `i++` does nothing.
  for (let i = 0; i < 3 && n < 100; i++) n = n + i + 1;
  let k = 5;
  const before = k--;
  return s + " " + n + " " + before + " " + k + " " + (++k);
}
f(["a", "b"]).then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "ab 6 5 4 5"
    );
}

// Every binding pattern lowers through the iterator-record intrinsics, which the JS
// emitter runs as the WASM emitter does (#15011). Expected output is Node's.
#[test]
fn async_body_runs_array_and_object_patterns() {
    assert_eq!(
        run_web_source(
            r#"
const pairs: [string, number][] = [["a", 1], ["b", 2]];
async function forOfArray() { const out: string[] = []; for (const [k, v] of pairs) out.push(k + v); return out.join(","); }
async function constArray() { const [x, y] = [1, 2]; return x + y; }
async function forOfObject() { const out: number[] = []; for (const { n } of [{ n: 1 }, { n: 2 }]) out.push(n); return out.join(","); }
async function forOfEntries() { const out: string[] = []; for (const [k, v] of Object.entries({ a: 1 })) out.push(k + v); return out.join(","); }
async function forOfPlain() { let s = 0; for (const v of [1, 2, 3]) s += v; return s; }
function* g() { yield 1; yield 2; yield 3; }
let closed = 0;
function* h() { try { yield 1; yield 2; yield 3; } finally { closed++; } }
async function restFromGenerator() { const [a, ...rest] = g(); return a + "|" + rest.join(","); }
async function defaults() { const [p = 5, q = 6] = [1]; return p + "," + q; }
async function closesAfterPattern() { const before = closed; const [a] = h(); return a + " closed " + (closed - before); }
async function breakCloses() { const before = closed; for (const v of h()) { if (v === 2) break; } return "closed " + (closed - before); }
async function throwCloses() { const before = closed; try { for (const v of h()) { throw new Error("boom " + v); } } catch (e: any) { return e.message + ", closed " + (closed - before); } return "no throw"; }
async function fromGenerator() { const [a, b] = g(); return a + "," + b; }
function syncForOfArray() { const out: string[] = []; for (const [k, v] of pairs) out.push(k + v); return out.join(","); }
const cases: [string, () => any][] = [["for-of [k, v]", forOfArray], ["const [x, y] =", constArray], ["for-of { n }", forOfObject], ["for-of [k, v] of Object.entries", forOfEntries], ["for-of v (no pattern)", forOfPlain], ["const [a, b] = generator", fromGenerator], ["sync for-of [k, v]", syncForOfArray], ["const [a, ...rest] = generator", restFromGenerator], ["const [p = 5, q = 6] =", defaults], ["const [a] = generator closes", closesAfterPattern], ["break closes", breakCloses], ["throw closes", throwCloses]];
async function main() { for (let i = 0; i < cases.length; i++) { const c = cases[i]; try { console.log(c[0] + ": " + (await c[1]())); } catch (e: any) { console.log(c[0] + ": threw " + e.message); } } }
main();
"#
        ),
        "for-of [k, v]: a1,b2\nconst [x, y] =: 3\nfor-of { n }: 1,2\nfor-of [k, v] of Object.entries: a1\nfor-of v (no pattern): 6\nconst [a, b] = generator: 1,2\nsync for-of [k, v]: a1,b2\nconst [a, ...rest] = generator: 1|2,3\nconst [p = 5, q = 6] =: 1,6\nconst [a] = generator closes: 1 closed 1\nbreak closes: closed 1\nthrow closes: boom 1, closed 1"
    );
}

// The `try` lowering declares a flag inside the `try` block and reads it in the `catch`;
// the JS body must declare it where both can see it (#14595).
#[test]
fn async_body_catch_reads_a_local_the_try_block_declared() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  try {
    throw new Error("boom");
  } catch (e) {
    return "caught " + (e as Error).message;
  }
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "caught boom"
    );
}

// An expression the JS emitter has no arm for must reject, not run on with `undefined`:
// a typed array has no arm in an async body, so iterating it throws.
#[test]
fn async_body_rejects_iteration_it_cannot_emit() {
    let out = run_web_source(
        r#"
async function each(): Promise<string> {
  let s = "";
  for (const v of new Uint8Array([120, 121])) s = s + v;
  return s;
}
each().then((v) => console.log(v)).catch((e) => console.log("threw " + e.name));
"#,
    );
    assert_eq!(out, "threw TypeError");
}

// A logical operator in an async body evaluates its left operand once, and an `await`
// in its right operand still runs in the async body.
#[test]
fn async_body_logical_operators_evaluate_the_left_operand_once() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  let i = 1;
  const a = i++ || 0;
  let j = 0;
  const b = j++ && 9;
  let k: any = 4;
  const c = k ?? 0;
  const d = (i > 100) || (await Promise.resolve(7));
  return i + "," + a + " " + j + "," + b + " " + c + " " + d;
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "2,1 1,0 4 7"
    );
}

#[test]
fn a_join_inside_a_call_argument_keeps_the_other_arguments() {
    assert_eq!(
        run_web_source(
            r#"
class Box { items: number[] = [1, 2, 3]; }
const inst = new Box();
const n = 7;
console.log(n, inst.items.join(","));
console.log(inst.items.join("-"), inst instanceof Box);
console.log(n, inst.items.join());
const lit = { out: ["x", "y"] };
console.log(lit.out.join(","), lit instanceof Object);
"#
        ),
        "7 1,2,3\n1-2-3 true\n7 1,2,3\nx,y true"
    );
}

// `join` converts each element as Array.prototype.join does: a nested array joins with
// ",", null and undefined are empty strings, an undefined separator is ",". Expected
// output is Node's.
// `some`, `every`, `findLast` and `findLastIndex` on an array, which HIR folds into
// their own nodes, call the callback with (element, index, array). Expected output is
// Node's.
#[test]
fn folded_some_every_and_find_last_match_node() {
    assert_eq!(
        run_web_source(
            r#"
const a = [1, 2, 3];
const k = 2;
console.log(a.some((v) => v > 2), a.some((v, i) => v < i), a.every((v) => v > 0), a.every((v, i) => v > i + 1));
console.log(a.findLast((v) => v < 3), a.findLastIndex((v) => v < 3), a.findLast((v) => v > 9), a.findLastIndex((v) => v > 9));
console.log([].some((v) => true), [].every((v) => false), a.some((v) => v === k), a.findLast((v, i, arr) => arr.length === 3 && i === 0));
"#
        ),
        "true false true false\n2 1 undefined -1\nfalse true true 1"
    );
}

#[test]
fn join_converts_nested_arrays_and_nullish_elements_as_js_does() {
    assert_eq!(
        run_web_source(
            r#"
const nested = [[1, 2], [3]];
const mixed: any = [[1, [2, 3]], "s", null, undefined, [true], { a: 1 }];
function joinIn(xs: number[][]) { return xs.join(";"); }
console.log(nested.join(";") + " | " + nested.join() + " | " + mixed.join("|") + " | " + joinIn([[4], [5, 6]]) + " | " + [1, 2].join(";") + " | " + [1, 2].join(undefined));
"#
        ),
        "1,2;3 | 1,2,3 | 1,2,3|s|||true|[object Object] | 4;5,6 | 1;2 | 1,2"
    );
}

#[test]
fn an_array_bridge_call_opens_its_own_argument_frame() {
    // Only the neighbours are asserted: what these calls return is a separate matter.
    // Writing into the enclosing call's frame replaced them.
    let out = run_web_source(
        r#"
const n = 7;
const a = [3, 1, 2];
console.log(n, a.toReversed(), n);
console.log(n, a.toSorted(), n);
console.log(n, a.toSorted((x, y) => y - x), n);
console.log(n, a.with(0, 9), n);
console.log(n, a.toSpliced(1, 1), n);
console.log(n, a.keys(), n);
console.log(n, a.values(), n);
console.log(n, a.entries(), n);
"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 8, "{out}");
    for line in lines {
        assert!(line.starts_with("7 ") && line.ends_with(" 7"), "{out}");
    }
}

// `Toggle(label, onChange: (value: boolean) => void)` hands the callback a boolean.
#[test]
fn toggle_on_change_receives_a_boolean() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Toggle } from "perry/ui";
Toggle("Notify", (value: boolean) => { console.log("toggle " + (value === true) + " " + typeof value); });
"#,
            r#"
const toggle = [...uiHandles.values()].find(e => e.tagName === "LABEL");
toggle._inp.checked = true;
toggle._inp.dispatch("change");
"#
        ),
        "toggle true boolean"
    );
}

#[test]
fn a_class_declared_in_a_function_keeps_its_methods() {
    assert_eq!(
        run_web_source(
            r#"
function make(): any {
  class Local extends Error {
    who() { return "local"; }
  }
  return new Local("m");
}
const b = make();
console.log(typeof b.who);
console.log(b.who());
function plain(n: number): any {
  class P { v: number; constructor(v: number) { this.v = v; } twice() { return this.v * 2; } }
  return new P(n);
}
console.log(plain(4).twice());
"#
        ),
        "function\nlocal\n8"
    );
}

// A closure created before a class declaration it reads shares that binding's cell, so
// it sees the class once the declaration has run.
#[test]
fn a_closure_reads_a_class_declared_after_it() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) {
  const read = () => C;
  class C { t() { return tag; } }
  return read() === C;
}
function makeError() {
  const read = () => E;
  class E extends Error {}
  return typeof read() + " " + (read() === E);
}
console.log(make("a") + " " + make("b") + " " + makeError());
"#
        ),
        "true true function true"
    );
}

#[test]
fn an_object_literal_super_call_starts_at_the_literals_prototype() {
    // The methods are read off the object before the call, so the test does not
    // depend on how `o.m()` dispatches a closure property.
    assert_eq!(
        run_web_source(
            r#"
const base = { hi() { return "base"; }, greet(x: string) { return "hello " + x; } };
const o = { __proto__: base, hi() { return "o>" + super.hi(); }, greet(x: string) { return super.greet(x) + "!"; } };
const hi = o.hi;
const greet = o.greet;
console.log(hi());
console.log(greet("w"));
const k = "hi";
const p = { __proto__: o, hi() { return "p>" + super[k](); } };
const phi = p.hi;
console.log(phi());
const q = { __proto__: base, x: 1 };
const qhi = q.hi;
console.log(q.x, qhi());
"#
        ),
        "o>base\nhello w!\np>o>base\n1 base"
    );
}

// The lookup reads the literal's [[Prototype]] when the call runs, and before the
// arguments are evaluated; a null prototype or a non-callable property throws.
#[test]
fn an_object_literal_super_call_resolves_at_call_time() {
    assert_eq!(
        run_web_source(
            r#"
const a = { m() { return "a"; } };
const b = { m() { return "b"; } };
const o = { __proto__: a, m(x: string) { return super.m() + x; }, n() { return super.m(Object.setPrototypeOf(o, b)); } };
const om = o.m;
console.log(om("!"));
Object.setPrototypeOf(o, b);
console.log(om("!"));
Object.setPrototypeOf(o, a);
const on = o.n;
console.log(on());
const z = { __proto__: null, m() { return super.m(); } };
const zm = z.m;
try { zm(); console.log("null: no throw"); } catch (e) { console.log("null: " + e.name); }
const c = { __proto__: { m: 1 }, m() { return super.m(); } };
const cm = c.m;
try { cm(); console.log("1: no throw"); } catch (e) { console.log("1: " + e.name); }
"#
        ),
        "a!\nb!\na\nnull: TypeError\n1: TypeError"
    );
}

#[test]
fn a_closure_used_as_a_value_is_a_function() {
    assert_eq!(
        run_web_source(
            r#"
const f = () => 1;
function named(): number { return 2; }
console.log(typeof f + " " + typeof (() => 1) + " " + typeof named);
const o: any = {}; o.h = f;
console.log(typeof o.h + " " + o.h() + " " + ({ g(n: number) { return n + 1; } }).g(41));
const fake: any = { funcIdx: 0, captures: [] };
console.log(typeof fake);
"#
        ),
        "function function function\nfunction 1 42\nobject"
    );
}

#[test]
fn an_async_body_calls_a_closure_held_by_an_object() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = {}; o.h = (n: number) => n * 2;
async function main() { console.log(typeof o.h + " " + o.h(21)); }
main();
"#
        ),
        "function 42"
    );
}

#[test]
fn an_async_body_calls_a_closure_held_in_a_local() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = {}; o.h = (n: number) => n * 2;
async function main() {
  const f = o.h;
  if (typeof f === "function") console.log(f(21));
  const m: any = Math.max;
  console.log(m(3, 7));
  const from: any = Array.from;
  console.log(from([1, 2], o.h).join(","));
  const t = o.t;
  t();
}
o.t = () => { throw new Error("boom"); };
main().catch((e: any) => console.log("caught " + e.message));
"#
        ),
        "42\n7\n2,4\ncaught boom"
    );
}

#[test]
fn an_async_body_keeps_the_receiver_of_a_computed_method_call() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = { n: 2 }; o.h = (n: number) => n * 2;
async function main() {
  const a = [1, 2];
  const k = "join";
  const h = "h";
  console.log(a[k]("-") + " " + o[h](21));
}
main();
"#
        ),
        "1-2 42"
    );
}

#[test]
fn an_async_body_passes_a_closure_lookalike_to_a_host_function_unchanged() {
    assert_eq!(
        run_web_source(
            r#"
const fake: any = { funcIdx: 0, captures: [] };
async function main() {
  const of: any = Array.of;
  console.log(typeof of(fake)[0]);
}
main();
"#
        ),
        "object"
    );
}

// Each evaluation of a capturing class expression is its own constructor, and its
// members read the captures of the evaluation that built the instance.
#[test]
fn each_class_expression_evaluation_keeps_its_captures() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) { return class { who() { return tag; } }; }
const A = make("A");
const B = make("B");
const a = new A();
const b = new B();
console.log(a.who() + " " + b.who() + " " + (A === B));
console.log((a instanceof A) + " " + (a instanceof B) + " " + (b instanceof B));
"#
        ),
        "A B false\ntrue false true"
    );
}

#[test]
fn a_class_expression_sees_a_capture_assigned_after_it() {
    assert_eq!(
        run_web_source(
            r#"
function counter(start: number) {
  let n = start;
  const K = class { bump() { n = n + 1; return n; } peek() { return n; } };
  n = start * 10;
  return K;
}
const C1 = counter(1);
const C2 = counter(2);
const c1 = new C1();
console.log(c1.peek() + " " + new C2().peek());
console.log(c1.bump() + " " + new C1().peek() + " " + new C2().bump());
"#
        ),
        "10 20\n11 11 21"
    );
}

// A method read off an instance or off `C.prototype` runs in that class's
// evaluation whatever its `this`, as a JS method closes over its class (#12578).
#[test]
fn a_method_value_runs_in_its_class_expression_evaluation() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) { return class { inst = tag; who() { return tag; } }; }
const A = make("A");
const B = make("B");
const a: any = new A();
const b: any = new B();
console.log(a.who.call(b) + " " + A.prototype.who.call(b) + " " + Reflect.apply(A.prototype.who, b, []));
const borrowed = { who: b.who, inst: "x" };
console.log(borrowed.who() + " " + b.inst);
"#
        ),
        "A A A\nB B"
    );
}

#[test]
fn a_class_declaration_in_a_function_is_fresh_per_call() {
    assert_eq!(
        run_web_source(
            r#"
function declared(t: string) {
  class K { v: number; constructor(v: number) { this.v = v; } who() { return t + this.v; } again() { return new K(this.v + 1); } }
  return new K(1);
}
const x = declared("x");
const y = declared("y");
console.log(x.who() + " " + y.who() + " " + x.again().who() + " " + y.again().who());
function bare() { return class {}; }
console.log(bare() === bare());
"#
        ),
        "x1 y1 x2 y2\nfalse"
    );
}

// `super.x` in a class method reads from the prototype of the method's home: a parent
// class's getter runs on the receiver, a class without `extends` reads Object.prototype,
// and `extends null` has no prototype, so the read throws.
#[test]
fn a_super_property_read_uses_the_home_prototype() {
    assert_eq!(
        run_web_source(
            r#"
class N extends null { m() { return super.x; } }
try { console.log(N.prototype.m.call({})); } catch (e) { console.log(e instanceof TypeError); }
class A { get g() { return "g" + this.k; } }
class B extends A { k = 1; m() { return super.g + " " + typeof super.toString + " " + super.nope; } }
console.log(new B().m());
"#
        ),
        "true\ng1 function undefined"
    );
}

// An evaluation's instance inherits its evaluation and class tag, so it gains no own key,
// whether the class value built it or a member's `new K()` on a later evaluation did.
#[test]
fn an_evaluation_instance_keeps_only_its_declared_own_keys() {
    assert_eq!(
        run_web_source_then(
            r#"
function make(tag: string) { return class { x = 1; who() { return tag; } }; }
function declared(t: string) { class K { v = 2; who() { return t; } again() { return new K(); } } return new K().again(); }
const g: any = globalThis;
const A = make("A");
const B = make("B");
g.a = new A();
g.b = new B();
declared("j");
g.k = declared("k");
console.log(g.a.who() + g.b.who() + g.k.who() + " " + (g.a instanceof A) + " " + (g.a instanceof B) + " " + (g.b instanceof B));
"#,
            r#"
for (const o of [a, b, k]) {
  console.log(Reflect.ownKeys(o).map(String).join(",") + "|" + [...new Headers(o).keys()].join(","));
}
"#
        ),
        "ABk true false true\nx|x\nx|x\nv|v"
    );
}

// Node's `Headers` copies every own key, enumerable or not: an instance or object literal
// built in sync code or in an async (JS) body shows it only declared fields.
#[test]
fn an_instance_has_no_own_class_tag_key() {
    assert_eq!(
        run_web_source_then(
            r#"
class P { x: number; constructor() { this.x = 1; } }
const g: any = globalThis;
g.h = { "Content-Type": "application/json" };
g.p = new P();
async function inBody(): Promise<void> { g.ah = { "X-A": "1" }; g.ap = new P(); }
inBody();
"#,
            r#"
await new Promise((r) => setTimeout(r, 0));
for (const o of [h, p, ah, ap]) {
  console.log(Reflect.ownKeys(o).join(",") + "|" + [...new Headers(o).keys()].join(","));
}
"#
        ),
        "Content-Type|content-type\nx|x\nX-A|x-a\nx|x"
    );
}

// A copy of an instance is an untagged plain object; an object literal's prototype is
// Object.prototype.
#[test]
fn a_copied_instance_is_untagged_and_the_original_still_dispatches() {
    assert_eq!(
        run_web_source_then(
            r#"
class Q { v: number; constructor() { this.v = 2; } get twice(): number { return this.v * 2; } }
const q = new Q();
const c: any = { ...q };
const a: any = Object.assign({}, q);
const g: any = globalThis;
g.q = q; g.c = c; g.lit = { v: 3 };
console.log(JSON.stringify(c) + " " + (c instanceof Q) + " " + (a instanceof Q) + " " + (q instanceof Q) + " " + q.twice);
"#,
            r#"console.log([lit, c, q].map((o) => Object.getPrototypeOf(o) === Object.prototype).join(" "));"#
        ),
        "{\"v\":2} false false true 4\ntrue true false"
    );
}

#[test]
fn an_evaluation_prototype_is_the_one_its_instances_inherit() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) { return class { who() { return tag; } }; }
const A: any = make("A");
A.prototype.extra = 7;
const a: any = new A();
console.log(a.extra + " " + a.who());
"#
        ),
        "7 A"
    );
}

#[test]
fn reflect_apply_takes_an_array_like_argument_list() {
    assert_eq!(
        run_web_source(
            r#"
class A { echo(x: number) { return x; } }
console.log(Reflect.apply(A.prototype.echo, new A(), { 0: 7, length: 1 } as any));
"#
        ),
        "7"
    );
}

// A call with spread arguments passes the elements, in a sync and in an async body: to a
// function (a void one too), a closure, a method (a computed key too), `push`, `Math.max` /
// `Math.min`, a host static and `console.log`. Expected output is Node's.
#[test]
fn a_spread_call_passes_the_spread_elements() {
    assert_eq!(
        run_web_source(
            r#"
function sum3(a: number, b: number, c: number): number { return a + b + c; }
const add = (a: number, b: number) => a + b;
class K { base = 100; plus(a: number, b: number) { return this.base + a + b; } }
function note(a: number, b: number): void { console.log("note " + (a + b)); }
const o: any = { base: 10, m(a: number, b: number) { return this.base + a + b; } };
const xs: number[] = [1, 2, 3];
const anyXs: any = [4, 5, 6];
const arr: number[] = [0];
const n = arr.push(...xs, 9);
console.log(sum3(...(xs as [number, number, number])) + " " + sum3(0, ...(xs.slice(1) as [number, number])) + " " + add(...(xs as [number, number])) + " " + new K().plus(...(xs as [number, number])));
console.log(n + " " + arr.join(",") + " " + Math.max(...xs) + " " + Math.min(...anyXs) + " " + Math.max(0, ...xs) + " " + String.fromCharCode(...[72, 105]));
console.log(...["a", "b"], 1);
note(...(xs as [number, number]));
const key = "m";
console.log(o[key](...(xs as [number, number])));
async function inAsync(): Promise<string> { const ys = [1, 2, 3]; const a2: number[] = []; a2.push(...ys); note(...(ys as [number, number])); return Math.max(...ys) + ":" + a2.length + ":" + sum3(...(ys as [number, number, number])) + ":" + add(...(ys as [number, number])) + ":" + o[key](...(ys as [number, number])); }
inAsync().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "6 5 3 103\n5 0,1,2,3,9 3 4 3 Hi\na b 1\nnote 3\n13\nnote 3\n3:3:6:3:13"
    );
}

// A spread call on a UI handle reaches the UI method, as a call with fixed arguments does.
#[test]
fn a_spread_call_on_a_ui_handle_runs_the_ui_method() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Text } from "perry/ui";
const t: any = Text("hi");
t.setWidth(...[10]);
"#,
            r#"
const el = [...uiHandles.values()].find(e => e.textContent === "hi");
console.log(el.style.width);
"#
        ),
        "10px"
    );
}

// A static method called with spread arguments receives the closure itself, not a host wrapper.
#[test]
fn a_spread_call_to_a_static_method_passes_closures_unchanged() {
    assert_eq!(
        run_web_source(
            r#"
const g = () => 1;
class C { static same(f: any) { return f === g; } }
console.log(C.same(g) + " " + C.same(...[g]));
"#
        ),
        "true true"
    );
}

// B's setter-only `x` shadows A's getter, so `super.x` from C reads undefined.
#[test]
fn a_super_read_stops_at_a_setter_only_accessor() {
    assert_eq!(
        run_web_source(
            r#"
class A { get x() { return "a"; } }
class B extends A { set x(v: string) {} }
class C extends B { m() { return super.x; } }
console.log(new C().m());
"#
        ),
        "undefined"
    );
}

#[test]
fn an_arrow_function_reads_the_this_it_was_created_with() {
    assert_eq!(
        run_web_source(
            r#"
class B {
  children: any;
  n = 1;
  add = (c: any) => { this.children = c; };
  nested = () => { const f = () => this.n * 10; return f(); };
  later() { return () => this.n + 1; }
  count(a: number) { let c = 0; const inc = (d: number) => { c += this.n + a + d; }; inc(10); return c; }
}
const b = new B();
b.add([7]);
const detached = b.add;
console.log(b.children[0]);
detached("d");
console.log(b.children);
console.log(b.nested());
const f = b.later();
console.log(f());
console.log(b.count(2));
"#
        ),
        "7\nd\n10\n2\n13"
    );
}

#[test]
fn a_function_expression_called_as_a_method_reads_its_receiver() {
    assert_eq!(
        run_web_source(
            r#"
const o = { k: 5, g() { return this.k; } };
console.log(o.g());
const p: any = { name: "p", hi: function () { return "hi " + this.name; } };
const q: any = { name: "q" };
q.hi = p.hi;
console.log(p.hi() + "/" + q.hi());
const counter = { c: 0, inc() { this.c = this.c + 1; return this; } };
counter.inc().inc();
console.log(counter.c);
const r: any = { v: 3, outer() { return () => this.v; } };
console.log(r.outer()());
const holder = { k: 9, g2: o.g };
console.log(holder.g2());
"#
        ),
        "5\nhi p/hi q\n2\n3\n9"
    );
}

#[test]
fn a_parameter_default_reads_the_method_receiver() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = { k: 4, f: function (x = this.k) { return x; } };
console.log(o.f());
console.log(o.f(1));
"#
        ),
        "4\n1"
    );
}

#[test]
fn reflect_apply_hands_this_to_a_function_expression() {
    assert_eq!(
        run_web_source(
            r#"
const hi = function (this: any, p: string) { return p + this.name; };
console.log(Reflect.apply(hi, { name: "o" }, ["hi "]));
"#
        ),
        "hi o"
    );
}

#[path = "class_eval_tests.rs"]
mod class_eval;

#[path = "array_copy_method_tests.rs"]
mod array_copy_method;

#[path = "collection_for_of_tests.rs"]
mod collection_for_of;

#[path = "static_method_tests.rs"]
mod static_method;

#[path = "date_setter_tests.rs"]
mod date_setter;

#[path = "class_value_tests.rs"]
mod class_value;

#[path = "weak_ref_tests.rs"]
mod weak_ref;

#[path = "function_value_tests.rs"]
mod function_value;

#[path = "get_prototype_of_tests.rs"]
mod get_prototype_of;

#[path = "object_is_tests.rs"]
mod object_is;

#[path = "object_create_tests.rs"]
mod object_create;

#[path = "object_prototype_method_tests.rs"]
mod object_prototype_method;

#[path = "class_prototype_tests.rs"]
mod class_prototype;

#[path = "builtin_subclass_tests.rs"]
mod builtin_subclass;

#[path = "array_from_tests.rs"]
mod array_from;

#[path = "rest_param_tests.rs"]
mod rest_param;

#[path = "thread_callback_tests.rs"]
mod thread_callback;

#[path = "math_minmax_tests.rs"]
mod math_minmax;

#[path = "async_operator_tests.rs"]
mod async_operator;

#[path = "patched_builtin_tests.rs"]
mod patched_builtin;

#[test]
fn textfield_set_input_type_sets_the_input_type() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { SecureField, TextField, textfieldSetInputType } from "perry/ui";
const phone = TextField("phone", (v: string) => {});
textfieldSetInputType(phone, "tel");
const email = TextField("email", (v: string) => {});
textfieldSetInputType(email, "email");
const count = TextField("count", (v: string) => {});
textfieldSetInputType(count, "number");
const box = TextField("box", (v: string) => {});
textfieldSetInputType(box, "tel");
textfieldSetInputType(box, "checkbox");
const pin = SecureField("pin", (v: string) => {});
textfieldSetInputType(pin, "number");
"#,
            r#"
for (const name of ["phone", "email", "count", "box", "pin"]) {
  const el = [...uiHandles.values()].find(e => e.placeholder === name);
  console.log(name + " " + el.type);
}
"#
        ),
        "phone tel\nemail email\ncount number\nbox text\npin password"
    );
}

#[path = "array_iterator_tests.rs"]
mod array_iterator;
