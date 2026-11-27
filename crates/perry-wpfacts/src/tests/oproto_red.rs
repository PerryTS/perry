//! OPROTO1/OPROTO2 fixtures: `ObjectProto` taken as a VALUE
//! is collapsed with every builtin prototype. Each test asserts the solve
//! creates no unknown value (first TopStop block == "NO STOP"); the red
//! ones fail on base because a read on the collapsed token gives the opaque
//! builtin function (`intrinsic_proto_own` -> `undescribed_member`) or a
//! spurious `ObjectProto` value (`proto_value_at`, `family_proto_value`).
use crate::{compute, FactsConfig};

fn first_block(src: &str) -> String {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::topstop::capture_on();
    let _ = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let v = crate::topstop::take_captured();
    v.into_iter().next().unwrap_or_else(|| "NO STOP".to_string())
}

const CTX: &str = "class Box { n = 0; set(v: number): void { this.n = v; } }\n\
class Sub extends Box { get2(): number { return this.n; } }\n\
const o = new Sub();\n\
function fn(this: unknown, x: unknown): unknown { return x; }\n\
const sink: unknown[] = [];\n";

const USE: &str = "o.set(1); console.log(o.get2(), sink.length);\n";

fn check(tag: &str, body: &str) {
    let b = first_block(&format!("{CTX}{body}{USE}"));
    eprintln!("=== OPROTO {tag}\n{}\n=== END {tag}", b.lines().take(12).collect::<Vec<_>>().join("\n"));
    assert!(b == "NO STOP", "OPROTO {tag}: {}", b.lines().next().unwrap_or(""));
}

/// O1 (HRAISE probe B): walk a class chain to its end (Object.prototype,
/// no `p !== Object.prototype` stop), call each own method value on `o`.
#[test]
fn oproto_o1_chain_walk_descriptor_apply() {
    check("O1", "for (let p = Object.getPrototypeOf(o); p !== null; p = Object.getPrototypeOf(p)) {\n\
  for (const k of Object.getOwnPropertyNames(p)) {\n\
    if (k === \"constructor\") continue;\n\
    const d = Object.getOwnPropertyDescriptor(p, k);\n\
    if (d === undefined || typeof d.value !== \"function\") continue;\n\
    const f = d.value as (...a: unknown[]) => unknown;\n\
    try { sink.push(f.apply(o, [1])); } catch (e) { sink.push(e); }\n\
  }\n\
}\n");
}

/// O2: computed-key read on `Object.prototype` itself, called on `o`.
#[test]
fn oproto_o2_computed_read_on_object_prototype() {
    check("O2", "for (const k of Object.getOwnPropertyNames(Object.prototype)) {\n\
  if (k === \"constructor\" || k === \"__proto__\") continue;\n\
  const v = (Object.prototype as any)[k];\n\
  if (typeof v === \"function\") { try { sink.push(v.call(o)); } catch (e) { sink.push(e); } }\n\
}\n");
}

/// O3: the [[Prototype]] of `%Object.prototype%` is null: nothing to call.
#[test]
fn oproto_o3_proto_of_object_prototype_is_null() {
    check("O3", "const q = Object.getPrototypeOf(Object.prototype) as any;\n\
if (q !== null) { sink.push(q.foo(o)); }\n");
}

/// O4: a named member read on the chain end of a class (by value).
#[test]
fn oproto_o4_named_read_on_chain_end_value() {
    check("O4", "const P = Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(o))) as any;\n\
sink.push(P.toString.call(o));\n");
}

/// O5: `Function.prototype` reached by value (getPrototypeOf of a function).
#[test]
fn oproto_o5_function_prototype_by_value() {
    check("O5", "const FP = Object.getPrototypeOf(fn) as any;\n\
const h = FP.bind.call(fn, o, 2) as () => unknown;\n\
sink.push(h());\n");
}

/// O6: `Array.prototype` reached by value, computed own-key reads.
#[test]
fn oproto_o6_array_prototype_by_value_computed() {
    check("O6", "const AP = Object.getPrototypeOf([1, 2]) as any;\n\
for (const k of [\"push\", \"indexOf\"]) {\n\
  const v = AP[k];\n\
  if (typeof v === \"function\") sink.push(v.call([o], o));\n\
}\n");
}

/// O7: getPrototypeOf(Array.prototype) is exactly Object.prototype.
#[test]
fn oproto_o7_proto_of_array_prototype() {
    check("O7", "const OP = Object.getPrototypeOf(Array.prototype) as any;\n\
for (const k of Object.getOwnPropertyNames(OP)) {\n\
  if (k === \"constructor\" || k === \"__proto__\") continue;\n\
  const v = OP[k];\n\
  if (typeof v === \"function\") { try { sink.push(v.call(o)); } catch (e) { sink.push(e); } }\n\
}\n");
}

/// C1 (control, HRAISE probe A): the same walk with the T7 stop.
#[test]
fn oproto_c1_walk_with_t7_stop() {
    check("C1", "for (let p = Object.getPrototypeOf(o); p !== null && p !== Object.prototype; p = Object.getPrototypeOf(p)) {\n\
  for (const k of Object.getOwnPropertyNames(p)) {\n\
    if (k === \"constructor\") continue;\n\
    const d = Object.getOwnPropertyDescriptor(p, k);\n\
    if (d === undefined || typeof d.value !== \"function\") continue;\n\
    const f = d.value as (...a: unknown[]) => unknown;\n\
    try { sink.push(f.apply(o, [1])); } catch (e) { sink.push(e); }\n\
  }\n\
}\n");
}

/// C2 (control): named reads on the global `Object.prototype`.
#[test]
fn oproto_c2_named_reads_direct() {
    check("C2", "sink.push(Object.prototype.hasOwnProperty.call(o, \"n\"));\n\
sink.push(Object.prototype.toString.call(o));\n");
}

// ---- OPROTO2 soundness fixtures: a payload handed to unknown code `u`
// through a prototype reached by VALUE must escape (the split must not lose
// a value or a write). Each is green on base (collapsed token) too.

fn payload_escape(body: &str) -> Option<String> {
    let src = format!(
        "class Payload {{ pb = 1; }}\nconst payload = new Payload();\n\
const u: any = new Function(process.argv[2]);\n\
class D {{ m(x: any) {{ return x; }} }}\n\
function fn(this: unknown, x: unknown): unknown {{ return x; }}\n\
{body}\nconsole.log(payload.pb);\n"
    );
    let pf = crate::tests::facts(&src);
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == "pb"))
        .unwrap_or_else(|| panic!("no Payload template\n{}", pf.dump_text()))
        .escape
        .clone()
}

/// DEC 250 (`perry_abi::lock` `add_locked`): %Object.prototype% is
/// add-locked, so a program store of a new key on it throws a TypeError at
/// the store (runtime `builtin_proto.rs` `throw_locked`; PROTOW1 probes,
/// every store form), and the solver reads `Object.prototype` from the same
/// lock decision (`members::locked_read`): the stored function is never on
/// the chain, `q.hook` has no call target, and `payload` does not escape.
/// These flip back to `escapes` if owner 2026-11-16 item 3 (remove the
/// builtin-prototype store refusal) lands.
fn stays(tag: &str, body: &str) {
    let e = payload_escape(body);
    eprintln!("=== OPROTO {tag} escape={e:?}");
    assert!(e.is_none(), "OPROTO {tag}: payload must not escape (DEC 250 add lock): {e:?}");
    let src = format!(
        "class Payload {{ pb = 1; }}\nconst payload = new Payload();\n\
const u: any = new Function(process.argv[2]);\n\
class D {{ m(x: any) {{ return x; }} }}\n\
function fn(this: unknown, x: unknown): unknown {{ return x; }}\n\
{body}\nconsole.log(payload.pb);\n"
    );
    let pf = crate::tests::facts(&src);
    for s in pf.sites.iter().filter(|s| s.is_call && s.key == "hook") {
        assert!(s.targets.is_empty(), "OPROTO {tag}: hook call has targets {:?}", s.targets);
    }
}

fn escapes(tag: &str, body: &str) {
    let e = payload_escape(body);
    eprintln!("=== OPROTO {tag} escape={e:?}");
    assert!(e.is_some(), "OPROTO {tag}: payload must escape");
}

/// S0: a store onto Object.prototype by name throws (DEC 250, `stays`).
#[test]
fn oproto_s0_object_prototype_store_by_name() {
    stays("S0", "(Object.prototype as any).hook = u;\nconst q: any = {};\nq.hook(payload);\n");
    stays("S0b", "const OP: any = Object.prototype;\nOP.hook = u;\nconst q: any = {};\nq.hook(payload);\n");
}

/// S1: after `setPrototypeOf(f, Object.prototype)` the prototype of `f` is
/// %Object.prototype% itself: a store through it throws (DEC 250, `stays`).
#[test]
fn oproto_s1_set_proto_of_fn_to_object_prototype() {
    stays("S1", "function f() {}\nObject.setPrototypeOf(f, Object.prototype);\n\
const P: any = Object.getPrototypeOf(f);\nP.hook = u;\nconst q: any = {};\nq.hook(payload);\n");
}

/// S1b: the store onto %Object.prototype% throws (DEC 250) at run time, but
/// the solve books it, and the read `P.hook` on the prototype value itself
/// (`ObjectProto`, and `f`'s birth `%Function.prototype%` token, which the
/// flow-insensitive `setPrototypeOf` does not remove) answers the stored
/// `u` without the lock decision a `{}` lookup takes (`members::locked_read`).
/// Sound, imprecise; open (OPROTO4 report).
#[test]
#[ignore = "OPROTO4: DEC 250 expectation; red: a read on the prototype value itself answers the refused store (sound over-approximation)"]
fn oproto_s1b_object_prototype_store_read_through_set_proto() {
    stays("S1b", "function f() {}\nObject.setPrototypeOf(f, Object.prototype);\n\
(Object.prototype as any).hook = u;\nconst P: any = Object.getPrototypeOf(f);\nP.hook(payload);\n");
}

/// S2: a program write onto Array.prototype seen through getPrototypeOf([])
/// and the other way round.
#[test]
fn oproto_s2_array_prototype_write_by_value() {
    escapes("S2", "const AP: any = Object.getPrototypeOf([]);\nAP.hook = u;\n\
const a: any = [3];\na.hook(payload);\n");
    escapes("S2b", "(Array.prototype as any).hook = u;\n\
const AP: any = Object.getPrototypeOf([1]);\nAP.hook(payload);\n");
    escapes("S2d", "const OP: any = Object.getPrototypeOf(Object.getPrototypeOf(fn));\nOP.hook = u;\n\
const a: any = [1];\na.hook(payload);\n");
}

/// S2c: a store through getPrototypeOf(getPrototypeOf([])), %Object.prototype%,
/// throws (DEC 250, `stays`).
#[test]
fn oproto_s2c_object_prototype_write_by_value() {
    stays("S2c", "const OP: any = Object.getPrototypeOf(Object.getPrototypeOf([]));\nOP.hook = u;\n\
const q: any = {};\nq.hook(payload);\n");
}

/// S3: Object.create(proto) chains through builtin prototypes by value.
#[test]
fn oproto_s3_object_create_chains() {
    escapes("S3", "const b: any = Object.create(Object.getPrototypeOf([]));\n\
Object.getPrototypeOf(b).hook = u;\nconst a: any = [];\na.hook(payload);\n");
    escapes("S3b", "const b: any = Object.create(Object.getPrototypeOf(new Map()));\n\
(Map.prototype as any).hook = u;\nb.hook(payload);\n");
    escapes("S3c", "const b: any = Object.create(Object.getPrototypeOf(fn));\n\
(Object.prototype as any).hook = u;\nb.hook(payload);\n");
}

/// S4: a builtin receiver whose prototype the program replaced.
#[test]
fn oproto_s4_replaced_builtin_receiver_proto() {
    escapes("S4", "(D.prototype as any).hook = u;\nconst m = new Map();\nObject.setPrototypeOf(m, D.prototype);\n\
const P: any = Object.getPrototypeOf(m);\nP.hook(payload);\n");
    escapes("S4b", "function g() {}\nObject.setPrototypeOf(g, { hook: u });\n\
const P: any = Object.getPrototypeOf(g);\nP.hook(payload);\n");
    escapes("S4c", "function g() {}\nObject.setPrototypeOf(g, { hook: u });\n(g as any).hook(payload);\n");
}

/// S5: a store through the prototype of an Intl.Segmenter (by value) is
/// seen by another segmenter.
#[test]
fn oproto_s5_segmenter_prototype_write_by_value() {
    escapes("S5", "const SP: any = Object.getPrototypeOf(new (Intl as any).Segmenter());\nSP.hook = u;\n\
const s2: any = new (Intl as any).Segmenter();\ns2.hook(payload);\n");
}
