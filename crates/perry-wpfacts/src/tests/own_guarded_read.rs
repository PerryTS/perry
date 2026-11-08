//! A computed read `o[k]` that runs only after an own-property test of the
//! same object and key (`hasOwnProperty.call(o, k)`, `o.hasOwnProperty(k)`,
//! `Object.hasOwn(o, k)`, `propertyIsEnumerable.call(o, k)`) reads `o`'s own
//! properties only (`Cons::IdxOwnLoad`): never the prototype chain, so never
//! the inherited `constructor` (the Object constructor) or an
//! `Object.prototype` method. The test and the read name the same bindings,
//! nothing runs between them, the key holds no object (ToPropertyKey of an
//! object runs program code, twice) and the test's callee is only the
//! own-property builtin (`Cons::OwnGate`); otherwise the read is ordinary.

use super::*;

/// The verdicts of the call sites of method `key`.
fn calls(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| !matches!(v, Verdict::No(_)))
}

fn unproven(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// A dictionary of class instances under keys from the program's strings
/// (one of them `constructor`, which `Object.prototype` holds), then `body`
/// once per key `k`. Only `size()` is called as a method named `size`.
fn dict(body: &str) -> ProgramFacts {
    facts(&format!(
        r#"
        const hasOwnProperty = Object.prototype.hasOwnProperty;
        class Sym {{ n = 1; size(): number {{ return this.n; }} }}
        const dict: Record<string, Sym> = {{}};
        const names = ["a", "b"];
        for (const n of names) dict[n] = new Sym();
        const keys = ["a", "x", "constructor"];
        let total = 0;
        function other(): void {{ total += 2; }}
        for (const k of keys) {{
            {body}
        }}
        console.log(total);
        "#
    ))
}

const GET_PROPERTY: &str = r#"
    function getProperty<T>(map: Record<string, T>, key: string): T | undefined {
        return hasOwnProperty.call(map, key) ? map[key] : undefined;
    }
"#;

// ---- site tests: unproven on the base, proven with the rule ----

/// TypeScript's `getProperty` (src/compiler/core.ts): the conditional form
/// in a helper; the value read through it is a `Sym` or `undefined`.
#[test]
fn get_property_reads_own_values_only() {
    let pf = facts(&format!(
        r#"
        const hasOwnProperty = Object.prototype.hasOwnProperty;
        {GET_PROPERTY}
        class Sym {{ n = 1; size(): number {{ return this.n; }} }}
        const dict: Record<string, Sym> = {{}};
        for (const n of ["a", "b"]) dict[n] = new Sym();
        let total = 0;
        for (const k of ["a", "x", "constructor"]) {{
            const s = getProperty(dict, k);
            if (s !== undefined) total += s.size();
        }}
        console.log(total);
        "#
    ));
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn and_form_reads_own_values_only() {
    let pf = dict("if (hasOwnProperty.call(dict, k) && dict[k].size() > 0) total += 1;");
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn if_form_reads_own_values_only() {
    let pf = dict("if (hasOwnProperty.call(dict, k)) { const s = dict[k]; total += s.size(); }");
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn object_has_own_reads_own_values_only() {
    let pf = dict("total += Object.hasOwn(dict, k) ? dict[k].size() : 0;");
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// `o.hasOwnProperty(k)`: the member is `Object.prototype`'s when `o`'s
/// own keys cannot name it (a literal with named keys only).
#[test]
fn has_own_property_method_reads_own_values_only() {
    let pf = dict(
        "const lit: Record<string, Sym> = { a: new Sym(), b: new Sym() }; \
         total += lit.hasOwnProperty(k) ? lit[k].size() : 0;",
    );
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn property_is_enumerable_reads_own_values_only() {
    let pf =
        dict("total += Object.prototype.propertyIsEnumerable.call(dict, k) ? dict[k].size() : 0;");
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// The assignment target of `copyProperties` / `clone` is evaluated first,
/// but only bindings and a literal key: the read is still the first thing
/// that can run code.
#[test]
fn read_after_store_target_bindings_is_guarded() {
    let pf = dict(
        "const out: Sym[] = []; \
         if (hasOwnProperty.call(dict, k)) { out[0] = dict[k]; total += out[0].size(); }",
    );
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

// ---- twins: the read stays ordinary ----

/// `o.hasOwnProperty` of a dictionary written under a key no constant
/// names may be the dictionary's own value: the test is not the builtin's.
#[test]
fn has_own_property_member_of_a_dictionary_stays() {
    let pf = dict(
        "dict[process.argv[2]] = new Sym(); \
         total += dict.hasOwnProperty(k) ? dict[k].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// Written under the constants `"a"` and `"b"` only (`Node::strs`), the
/// dictionary owns no `hasOwnProperty`: the member is the builtin's.
#[test]
fn has_own_property_of_a_dictionary_of_constant_keys_is_the_builtin() {
    let pf = dict("total += dict.hasOwnProperty(k) ? dict[k].size() : 0;");
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// The read is not the first thing the branch runs (`values.push` is read
/// first: TypeScript's `getOwnValues`).
#[test]
fn member_read_before_stays() {
    let pf = dict(
        "const vals: Sym[] = []; \
         if (hasOwnProperty.call(dict, k)) { vals.push(dict[k]); } \
         for (const v of vals) total += v.size();",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn unguarded_read_stays() {
    let pf = dict("total += dict[k] ? dict[k].size() : 0;");
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn guard_on_another_object_stays() {
    let pf = dict(
        "const o2: Record<string, Sym> = {}; \
         total += hasOwnProperty.call(o2, k) ? dict[k].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn guard_on_another_key_stays() {
    let pf =
        dict("const k2 = k + \"!\"; total += hasOwnProperty.call(dict, k2) ? dict[k].size() : 0;");
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn key_reassigned_between_stays() {
    let pf = dict(
        "let kk = k; if (hasOwnProperty.call(dict, kk)) { kk = \"constructor\"; total += dict[kk].size(); }",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn call_between_stays() {
    let pf = dict("if (hasOwnProperty.call(dict, k)) { other(); total += dict[k].size(); }");
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// `in` passes inherited keys: `"constructor" in dict` is true.
#[test]
fn in_guard_stays() {
    let pf = dict("total += k in dict ? dict[k].size() : 0;");
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// The test's callee may be a program function: an ordinary read.
#[test]
fn program_callee_stays() {
    let pf = dict(
        "let h: any = hasOwnProperty; if (total > 5) h = function (this: any, key: string) { return true; }; \
         total += h.call(dict, k) ? dict[k].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// An object key converts by running its `toString`, at the test and again
/// at the read: the two keys may differ.
#[test]
fn object_key_stays() {
    let pf = dict(
        "let flip = false; \
         const ok: any = { toString() { flip = !flip; return flip ? k : \"constructor\"; } }; \
         total += hasOwnProperty.call(dict, ok) ? dict[ok].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// A guard through a body-derived own-test helper.
#[test]
fn helper_guard_reads_own_values_only() {
    let pf = dict(
        "function hasProperty(map: object, key: string): boolean { return hasOwnProperty.call(map, key); } \
         total += hasProperty(dict, k) ? dict[k].size() : 0;",
    );
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

// ---- OWN2: twins that try to break the rule ----

/// The program writes `Function.prototype.call`: the runtime refuses the
/// store (TypeError: a member of a locked intrinsic prototype, DEC 250), so
/// `H.call(o, k)` still runs the native and the read stays own-only.
#[test]
fn written_function_prototype_call_is_refused() {
    let pf = dict(
        "(Function.prototype as any).call = function (this: any, o: any, key: string) { return true; }; \
         total += hasOwnProperty.call(dict, k) ? dict[k].size() : 0;",
    );
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// The program writes an own `call` onto the builtin `hasOwnProperty`.
#[test]
fn written_own_call_of_the_builtin_stays() {
    let pf = dict(
        "(hasOwnProperty as any).call = function (o: any, key: string) { return true; }; \
         total += hasOwnProperty.call(dict, k) ? dict[k].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// The program writes `call` onto the builtin `hasOwnProperty` under a
/// computed key.
#[test]
fn written_computed_call_of_the_builtin_stays() {
    let pf = dict(
        "const names = [\"call\", \"q\"]; \
         for (const nm of names) (hasOwnProperty as any)[nm] = function (o: any, key: string) { return true; }; \
         total += hasOwnProperty.call(dict, k) ? dict[k].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// The program tries to replace `Object.prototype.hasOwnProperty` before
/// reading it: DEC 250 refuses the store (an image member of the locked
/// `Object.prototype`; the runtime throws a TypeError there), so the read
/// after it, if any runs, is the builtin.
#[test]
fn refused_object_prototype_has_own_property_store_leaves_the_builtin() {
    let pf = facts(
        r#"
        (Object.prototype as any).hasOwnProperty = function (key: string) { return true; };
        const hasOwnProperty = Object.prototype.hasOwnProperty;
        class Sym { n = 1; size(): number { return this.n; } }
        const dict: Record<string, Sym> = {};
        for (const n of ["a", "b"]) dict[n] = new Sym();
        const lit: Record<string, Sym> = { a: new Sym(), b: new Sym() };
        let total = 0;
        for (const k of ["a", "x", "constructor"]) {
            total += hasOwnProperty.call(dict, k) ? dict[k].size() : 0;
            total += lit.hasOwnProperty(k) ? lit[k].size() : 0;
        }
        console.log(total);
        "#,
    );
    assert!(!unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// The program replaces `Object.hasOwn` (a static member of a builtin
/// constructor is not locked).
#[test]
fn written_object_has_own_stays() {
    let pf = dict(
        "(Object as any).hasOwn = function (o: any, key: string) { return true; }; \
         total += Object.hasOwn(dict, k) ? dict[k].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// The object's own `hasOwnProperty` field, written under a named key.
#[test]
fn own_has_own_property_field_stays() {
    let pf = dict(
        "const lit: any = { a: new Sym(), b: new Sym() }; \
         lit.hasOwnProperty = () => true; \
         total += lit.hasOwnProperty(k) ? lit[k].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// The object binding is reassigned inside the guarded region.
#[test]
fn object_reassigned_between_stays() {
    let pf = dict(
        "let d: any = dict; \
         if (hasOwnProperty.call(d, k)) { d = Object.create(dict); total += d[k].size(); }",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// An own getter runs at the read: its result is the read's value.
#[test]
fn own_getter_runs() {
    let pf = dict(
        "const lit: any = { get a(): any { return Object; }, b: new Sym() }; \
         total += hasOwnProperty.call(lit, k) ? lit[k].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// An own symbol-keyed property is read under a symbol key.
#[test]
fn own_symbol_key_is_read() {
    let pf = dict(
        "const sym = Symbol(\"s\"); const lit: any = { a: new Sym() }; lit[sym] = Object; \
         const ks: any[] = [k, sym]; \
         for (const kk of ks) total += hasOwnProperty.call(lit, kk) ? lit[kk].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// An own numeric-keyed property of a program object under a number key.
#[test]
fn own_numeric_key_is_read() {
    let pf = dict(
        "const lit: any = { a: new Sym() }; lit[1] = Object; \
         const ks: any[] = [k, 1]; \
         for (const kk of ks) total += hasOwnProperty.call(lit, kk) ? lit[kk].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// A class instance: the key `size` lives on the class prototype only (the
/// test is false there); an own property `Object.assign` adds is read.
#[test]
fn instance_own_property_from_assign_is_read() {
    let pf = dict(
        "class Box { n = 1; size(): number { return this.n; } } \
         const b: any = new Box(); Object.assign(b, { x: Object }); \
         const ks = [k, \"size\", \"x\"]; \
         for (const kk of ks) total += hasOwnProperty.call(b, kk) ? b[kk].size() : 0;",
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// An object that reached unknown code: its own fields may hold anything.
#[test]
fn escaped_object_stays() {
    let pf = facts(
        r#"
        declare function ext(x: any): void;
        const hasOwnProperty = Object.prototype.hasOwnProperty;
        class Sym { n = 1; size(): number { return this.n; } }
        const lit: Record<string, Sym> = { a: new Sym(), b: new Sym() };
        ext(lit);
        let total = 0;
        for (const k of ["a", "x", "constructor"]) {
            total += hasOwnProperty.call(lit, k) ? lit[k].size() : 0;
        }
        console.log(total);
        "#,
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

/// `Object.call(o, k)` runs the `Object` constructor (`Object(k)`, an
/// object: always true): the `Object` constructor is not an own-property
/// builtin at a callee.
#[test]
fn object_constructor_call_stays() {
    let pf = dict("total += (Object as any).call(dict, k) ? dict[k].size() : 0;");
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}
