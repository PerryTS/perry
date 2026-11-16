//! DECISIONS 234 for method calls: looking up a method's callee is a
//! [[Get]], so an own definite data key stops it at the object exactly as
//! for a read. A store into ⊤ under a computed key (or an
//! `Object.setPrototypeOf` on ⊤) may write any key of `Object.prototype`;
//! the call `factory.createIdentifier(…)` on a literal that holds
//! `createIdentifier` from birth runs the own method (node 26: with
//! `Object.prototype.createIdentifier = () => 0`, the own method still runs)
//! and neither finds that unknown member nor lets the literal escape as its
//! receiver. Whatever revokes the shadow of a read revokes it for a call
//! (`flush_shadowed` binds the builtin members it skipped).

use super::*;

/// `h[name] = 1` may write any key of `Object.prototype` (node: `hostFn`
/// returns `Object.prototype`).
const CST: &str = r#"
    declare const name: string;
    const h: any = (globalThis as any).hostFn();
    h[name] = 1;
"#;

/// `Object.setPrototypeOf` on ⊤: ⊤ may be any object, its prototype any
/// builtin prototype's key holder (the E6 shape of TypeScript's
/// `createNodeArray`, debug.ts `Object.setPrototypeOf(array, proto)`).
const NOG: &str = r#"
    declare const name: string;
    const p = { tag: 7 };
    const q: any = (globalThis as any).hostFn();
    Object.setPrototypeOf(q, p);
"#;

/// The shape of TypeScript's `createNodeFactory`: a literal of methods,
/// returned from a function, called through its own keys.
const FACTORY: &str = r#"
    function createFactory(flags: number) {
        const factory = {
            flags,
            createIdentifier(text: string) { return { kind: 80, text }; },
        };
        return factory;
    }
    const factory: any = createFactory(4);
"#;

/// The call `factory.createIdentifier(…)` and the read `factory.flags`
/// after it (⊤ once the literal escaped as the call's receiver).
const USE: &str = r#"
    console.log(factory.createIdentifier("a").kind, factory.flags);
"#;

pub(super) fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

pub(super) fn call_of<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    pf.sites
        .iter()
        .find(|s| s.key == key && s.is_call)
        .unwrap_or_else(|| panic!("call site .{key}() in\n{}", pf.dump_text()))
}

/// Whole program: is the call proven, and the read of `flags` after it?
pub(super) fn whole(src: &str) -> (bool, bool) {
    let pf = facts(src);
    let call = proven(&call_of(&pf, "createIdentifier").verdict);
    (call, proven(&site(&pf, "flags").verdict))
}

/// Per unit (one module): is the call proven?
pub(super) fn per_unit(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    proven(&call_of(&pf, "createIdentifier").verdict)
}

#[test]
fn an_own_method_shadows_a_top_written_prototype_for_a_call() {
    let src = format!("{CST}{FACTORY}{USE}");
    assert_eq!(whole(&src), (true, true));
    assert!(per_unit(&src));
}

#[test]
fn an_own_method_shadows_a_prototype_set_on_top_for_a_call() {
    let src = format!("{NOG}{FACTORY}{USE}");
    assert_eq!(whole(&src), (true, true));
    assert!(per_unit(&src));
}

/// node: `hostFn(factory)` hands the literal out and `hostGet()` returns
/// it; the computed delete through ⊤ may remove `createIdentifier`, so the
/// call may run what the computed store put on `Object.prototype`.
#[test]
fn an_escaped_literal_with_a_delete_through_top_stays_unproven() {
    let src = format!(
        "{CST}{FACTORY}(globalThis as any).hostFn(factory); \
         delete (globalThis as any).hostGet()[name]; {USE}"
    );
    assert!(!whole(&src).0);
    assert!(!per_unit(&src));
}

/// A computed delete on the literal (through a parameter of a function
/// called after the call) may remove `createIdentifier`.
#[test]
fn a_delete_on_the_literal_revokes_the_shadow() {
    let src = format!(
        "{CST}{FACTORY}{USE} function zap(o: any, k: string) {{ delete o[k]; }} \
         zap(factory, name); {USE}"
    );
    assert!(!whole(&src).0);
}

/// The revocation comes after the calls in solve order: the value
/// `defineProperty` redefines a key of is the call's own result, which
/// exists only once the call is bound (with the shadow). The key no
/// longer shadows (`reshape`), and only `flush_shadowed` gives both calls
/// the builtin chain's unknown member. Without the flush they stay proven.
#[test]
fn a_define_property_through_top_after_the_call_revokes_the_shadow() {
    let calls = |redefine: &str| -> Vec<bool> {
        let src = format!(
            "{CST}
            function createFactory(flags: number) {{
                const factory = {{
                    flags,
                    createIdentifier(text: string) {{
                        return {{ kind: 80, text, host: (globalThis as any).hostGet() }};
                    }},
                }};
                return factory;
            }}
            const factory: any = createFactory(4);
            const r = factory.createIdentifier(\"a\");
            {redefine}
            {USE}"
        );
        let pf = facts(&src);
        pf.sites
            .iter()
            .filter(|s| s.key == "createIdentifier" && s.is_call)
            .map(|s| proven(&s.verdict))
            .collect()
    };
    // The fixture is proven without the redefinition.
    assert_eq!(calls("console.log(r.host);"), vec![true, true]);
    assert_eq!(
        calls("Object.defineProperty(r.host, name, { value: 1 });"),
        vec![true, true]
    );
    // DEC 250 Q2: the runtime refuses a new key on `Object.prototype` (TypeError), so the store into ⊤ wrote nothing there: `createIdentifier` is no key of it.
}

/// A key added after birth is no definite own key: the call still finds
/// the unknown member the store into ⊤ may have put on `Object.prototype`.
#[test]
fn a_key_added_after_birth_does_not_shadow_for_a_call() {
    let src = format!(
        "{CST} const factory: any = {{ flags: 4 }}; \
         factory.createIdentifier = (text: string) => ({{ kind: 80, text }}); {USE}"
    );
    assert!(!whole(&src).0);
    assert!(!per_unit(&src));
}

/// Without a write through ⊤ the call is proven, with the shadow or
/// without (the fixture is not red for another reason).
#[test]
fn without_a_top_write_the_call_is_proven() {
    let src = format!("{FACTORY}{USE}");
    assert_eq!(whole(&src), (true, true));
}

/// The calls of `createIdentifier` where `then` revokes the shadow after
/// the first call is bound: it acts on what the call's result holds
/// (`more`, fields of the result), which exists only once the call is.
fn revoked_calls(more: &str, then: &str) -> Vec<String> {
    let src = format!(
        "{CST}
        function createFactory(flags: number) {{
            const factory = {{
                flags,
                createIdentifier(text: string) {{
                    return {{ kind: 80, text, {more} }};
                }},
            }};
            return factory;
        }}
        const factory: any = createFactory(4);
        const r = factory.createIdentifier(\"a\");
        {then}
        {USE}"
    );
    // The lock admits constructor writes, so revoking the own shadow
    // still exposes an arbitrary chain value and exercises the re-check.
    let src = src.replace("createIdentifier", "constructor");
    let pf = facts(&src);
    pf.sites
        .iter()
        .filter(|s| s.key == "constructor" && s.is_call)
        .map(|s| match &s.verdict {
            Verdict::No(why) => why.to_string(),
            _ => "proven".to_string(),
        })
        .collect()
}

/// The reason of a call the unknown member of the builtin chain reached:
/// only the re-check gives the call this reason (other revocations fail it
/// with their own, e.g. the delete's `hard: E7`), so a test that compares
/// only proven/unproven passes without the re-check in a release build.
const UNKNOWN_MEMBER: &str = "escape: receiver of a call of an unknown function value";

/// A class event (`note_inst`, `keys_unsure`): a computed delete on the
/// literal, reached through the call's result, may remove
/// `createIdentifier`. Only the re-check of the lookups on the literal's
/// class gives the calls the unknown member the store into ⊤ may have put
/// on `Object.prototype`.
#[test]
fn a_delete_on_the_literal_after_the_call_revokes_the_shadow() {
    let more = "leak: () => factory";
    assert_eq!(
        revoked_calls(more, "console.log(r.leak().flags);"),
        vec!["proven", "proven"]
    );
    assert_eq!(
        revoked_calls(more, "delete r.leak()[name]; console.log(r.leak().flags);"),
        vec![UNKNOWN_MEMBER, UNKNOWN_MEMBER]
    );
}

/// A key event (`reshape`, `top_reshape`): a named `defineProperty`
/// through ⊤ (reached through the call's result) may redefine
/// `createIdentifier` on a builtin prototype. The factory is a literal: its
/// birth defines the key (CreateDataProperty), which nothing on a
/// prototype intercepts, and the call finds that own data property first
/// (ECMA-262 10.1.8.1 OrdinaryGet), so the calls stay proven.
#[test]
fn a_named_define_through_top_after_the_call_keeps_the_literal_key() {
    let more = "host: (globalThis as any).hostGet()";
    assert_eq!(
        revoked_calls(more, "console.log(r.host);"),
        vec!["proven", "proven"]
    );
    assert_eq!(
        revoked_calls(
            more,
            "Object.defineProperty(r.host, \"createIdentifier\", { value: 1 });"
        ),
        vec!["proven", "proven"]
    );
}
