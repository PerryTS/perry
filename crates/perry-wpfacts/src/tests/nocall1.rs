//! NOCALL1 (ECMA-262 7.3.14 Call, step 2): calling a value that is an
//! ordinary object without [[Call]] (an object literal, a class prototype
//! object, an array) throws a TypeError before anything runs. Such a callee
//! runs no code, hands its receiver and arguments to no one and holds no
//! realm: an unrelated read stays proven. A function beside it in the
//! callee set is still bound. Array.prototype.toString (23.1.3.36) calls
//! `join` only if IsCallable, else it is Object.prototype.toString: a
//! string, no throw. A callee that may be host code (⊤) stays unknown.
//! Unknown values come from real unknowns (`process.argv`), never from
//! `declare const`.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn held(src: &str) -> bool {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m.realm_held
}

/// The verdicts of the call sites of `key`, in source order.
fn calls(src: &str, key: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("NOCALL1 {key} {src:?}: {v:?}");
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

const OBJ: &str = "const o = { m() { return 1; } };\n";

/// (a) and the model path: the callee set holds only objects without [[Call]].
fn no_call(body: &str) {
    let src = format!("{OBJ}{body}\nconst r = o.m();\nconsole.log(r);\n");
    let h = held(&src);
    let v = calls(&src, "m");
    assert!(
        !h && mono(&v),
        "a callee without [[Call]] only throws: held={h} {v:?}"
    );
}

#[test]
fn literal_callee_only_throws() {
    no_call("const g: any = o;\ntry { g(); } catch (e) {}")
}

#[test]
fn chosen_literal_callee_only_throws() {
    no_call("const g: any = process.argv[2] ? o : [1, 2];\ntry { g(o); } catch (e) {}")
}

#[test]
fn class_prototype_callee_only_throws() {
    no_call(
        "class C { n() { return 2; } }\nconst g: any = C.prototype;\ntry { g(o); } catch (e) {}",
    )
}

#[test]
fn method_callee_object_only_throws() {
    no_call("const h: any = { f: o };\ntry { h.f(o); } catch (e) {}")
}

/// Array.prototype.toString with a non-callable `join` (any-key store).
#[test]
fn array_tostring_noncallable_join() {
    no_call(
        "const a: any = [1, 2];\na[process.argv[2]] = o;\nconst s = String(a);\nconsole.log(s);",
    )
}

#[test]
fn array_tostring_method_noncallable_join() {
    no_call(
        "const a: any = [1, 2];\na[process.argv[2]] = o;\nconst s = a.toString();\nconsole.log(s);",
    )
}

/// (b) an object and a program function: the function is bound.
#[test]
fn function_beside_object_is_bound() {
    let src = format!(
        "{OBJ}const k = {{ q() {{ return 3; }} }};\nconst g: any = process.argv[2] ? o : function () {{ return k.q(); }};\ntry {{ g(); }} catch (e) {{}}\nconst r = o.m();\nconsole.log(r);\n"
    );
    assert!(!held(&src), "held");
    assert!(mono(&calls(&src, "m")), "o.m");
    let q = calls(&src, "q");
    assert!(mono(&q), "the function is bound as callee: {q:?}");
}

/// (c) KEEP: a callee that may be host code is unknown and holds the realm.
/// K8 (owner 2026-11-23, f0d8455cfb): a call of an unknown value holds the
/// realm only where code beside the program may name it (`realm_nameable`),
/// so this control runs beside an unseen module (`import "unseen-host"`).
#[test]
fn keep_host_callee_held() {
    let src = format!(
        "import \"unseen-host\";\n{OBJ}const g: any = (globalThis as any)[process.argv[2]];\ng(o);\nconst r = o.m();\nconsole.log(r);\n"
    );
    assert!(held(&src), "a host callee holds the realm");
    assert!(!mono(&calls(&src, "m")), "o escaped to unknown code");
}

/// r9b (R9BCOST1): the any-key pool gives `join` the value %TypedArray%.from;
/// called with the array as this (no constructor) it throws (23.2.2.1 step 2).
#[test]
fn typedarray_from_as_join_only_throws() {
    no_call("const a: any = [1, 2];\na[process.argv[2]] = Float32Array.from;\nconst s = String(a);\nconsole.log(s);")
}

#[test]
fn typedarray_from_value_call_only_throws() {
    no_call("const g: any = Float32Array.from;\ntry { g(o); } catch (e) {}")
}

#[test]
fn proto_in_any_key_pool_join_only_throws() {
    no_call("class C { n() { return 2; } }\nconst a: any = [1, 2];\nconst t: any = {};\nt[process.argv[2]] = C.prototype;\na[process.argv[3]] = t[process.argv[4]];\nconst s = String(a);\nconsole.log(s);")
}

/// KEEP: %TypedArray%.from with a constructor as this runs its steps
/// (unmodelled: unknown code may run).
/// K8 (owner 2026-11-23, f0d8455cfb): a call of an unknown value holds the
/// realm only where code beside the program may name it (`realm_nameable`),
/// so this control runs beside an unseen module (`import "unseen-host"`).
#[test]
fn keep_typedarray_from_on_constructor_held() {
    for call in [
        "const g: any = Float32Array.from;\nconst x = g.call(Float32Array, o);",
        "class F extends Float32Array {}\nconst x = F.from(o as any);",
        "const h: any = { from: Float32Array.from, C: Float32Array };\nconst x = h.from.call(h.C, o);",
    ] {
        let src = format!("import \"unseen-host\";\n{OBJ}{call}\nconsole.log(x);\nconst r = o.m();\nconsole.log(r);\n");
        assert!(held(&src), "a constructor as this: still unknown: {call:?}");
    }
}

/// 23.1.3.36 step 3: a non-callable `join` makes toString
/// Object.prototype.toString: the result is a String, its `length` is read.
#[test]
fn array_tostring_fallback_is_a_string() {
    let src = format!("{OBJ}const a: any = [1, 2];\na.join = o;\nconst s = a.toString();\nconsole.log(s.length);\n");
    let pf = one(&src);
    let v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "length")
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    eprintln!("NOCALL1 length {v:?}");
    assert!(
        !v.is_empty() && v.iter().all(|x| x != "Dead"),
        "the fallback String: {v:?}"
    );
}
