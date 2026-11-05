//! Missing members call only values supplied by the program.
use super::*;

fn check(body: &str, proven: bool) {
    check_at(body, "o.field", "field", proven);
}

fn check_at(body: &str, observe: &str, key: &str, proven: bool) {
    let src = [
        r#"
declare function external(x: any): void;
declare function unknown(): any;
const o: any = { field: 1 };
const N: any = Object;
function g(p: any) { p.field = 2; external(p); }
"#,
        body,
        &format!("\nconsole.log({observe}); // OBSERVE\n"),
    ]
    .concat();
    let marker = src.find("// OBSERVE").unwrap();
    let start = src[..marker].rfind('\n').unwrap() as u32;
    let pf = facts_with(&src, interp_cfg());
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == key && (start..marker as u32).contains(&s.byte_offset))
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "missing named observation: {}",
        pf.dump_text()
    );
    assert_eq!(
        !matches!(sites[0].verdict, Verdict::No(_)),
        proven,
        "{body}\n{:?}\n{}",
        sites[0].verdict,
        pf.dump_text()
    );
}

#[test]
fn string_or_object_prototype_method_missing_last_index_of() {
    check(
        r#"const dict: any = { text: "text" };
const str = dict[process.argv[2]];
str.lastIndexOf(o, 0);"#,
        true,
    );
}

#[test]
fn array_or_object_constructor_missing_for_each() {
    check_at(
        r#"o.result = { value: 1 };
const original: any = process.argv[2] ? [o.result] : Object;
original.forEach((value: any) => { o.result = value; });"#,
        "o.result.value",
        "value",
        true,
    );
}

#[test]
fn namespace_missing_member_does_not_escape_argument() {
    check("const M: any = Math; M.zzNoSuchMember(o);", true);
    check("N.zzNoSuchMember(o);", true);
}

#[test]
fn namespace_program_store_is_called() {
    check("Object.zap = g; N.zap(o);", false);
}

#[test]
fn namespace_inherited_program_store_is_called() {
    for parent in ["Function.prototype", "Object.prototype"] {
        for call in ["N.zap(o);", "const r = N.zap; r(o);"] {
            check(&format!("{parent}.zap = g; {call}"), true);
        }
    }
}

#[test]
fn namespace_unknown_key_store_is_called() {
    check("Object[process.argv[2]] = g; N.zap(o);", false);
    check("unknown()[process.argv[2]] = g; N.zap(o);", false);
}

#[test]
fn namespace_accessors_are_called() {
    check(
        "Object.defineProperty(Object, 'zap', { get() { return g; } }); N.zap(o);",
        false,
    );
    check("N.__defineGetter__('zap', () => g); N.zap(o);", false);
}

#[test]
fn namespace_replaced_chain_is_called() {
    check(
        "Object.setPrototypeOf(Object, { zap: g }); N.zap(o);",
        false,
    );
    check("Object.__proto__ = { zap: g }; N.zap(o);", false);
}

#[test]
fn dynamic_code_keeps_missing_member_open() {
    check("eval(process.argv[2]); N.zap(o);", false);
    check("new Function(process.argv[2])(); N.zap(o);", false);
}

#[test]
fn published_namespace_keeps_missing_member_open() {
    check("external(N); N.zap(o);", false);
}

#[test]
fn declared_and_undescribed_namespace_keep_their_models() {
    check(
        "Object.assign(o, { get field() { g(o); return 2; } });",
        false,
    );
    check("const P: any = process; P.zap(o);", false);
}

fn natives(body: &str, proven: bool) {
    for f in ["[1].map", "Object.prototype.hasOwnProperty"] {
        check(&format!("const f: any = {f}; {body}"), proven);
    }
}

#[test]
fn native_missing_member_does_not_escape_argument() {
    natives("f.zzNoSuchMember(o);", true);
}

#[test]
fn native_program_store_is_called() {
    natives("f.zap = g; f.zap(o);", false);
}

#[test]
fn native_alias_store_is_called() {
    check("const s = new Set(); const f: any = s.keys; const alias: any = s.values; alias.zap = g; f.zap(o);", false);
}

#[test]
fn native_inherited_program_stores_are_called() {
    // DEC 250: adds throw. The call must agree with the read even where
    // Native::Member conservatively keeps that read open.
    for (f, proven) in [
        ("[1].map", false),
        ("Object.prototype.hasOwnProperty", true),
    ] {
        for parent in ["Function.prototype", "Object.prototype"] {
            for call in ["f.zap(o);", "const r = f.zap; r(o);"] {
                check(
                    &format!("const f: any = {f}; {parent}.zap = g; {call}"),
                    proven,
                );
            }
        }
    }
}

#[test]
fn native_replaced_chain_is_called() {
    natives("Object.setPrototypeOf(f, { zap: g }); f.zap(o);", false);
    natives("f.__proto__ = { zap: g }; f.zap(o);", false);
}

#[test]
fn native_accessors_are_called() {
    natives(
        "Object.defineProperty(f, 'zap', { get() { return g; } }); f.zap(o);",
        false,
    );
    natives(
        "Object.defineProperty(Function.prototype, 'zap', { get() { return g; } }); f.zap(o);",
        false,
    );
    natives("f.__defineGetter__('zap', () => g); f.zap(o);", false);
}

#[test]
fn native_unknown_key_stores_are_called() {
    natives("f[process.argv[2]] = g; f.zap(o);", false);
    natives("unknown()[process.argv[2]] = g; f.zap(o);", false);
}

#[test]
fn published_native_keeps_missing_member_open() {
    natives("external(f); f.zap(o);", false);
}

#[test]
fn native_declared_members_keep_their_models() {
    natives("f.call(o, { toString() { g(o); return 'x'; } });", false);
    natives("f.bind(o)({ toString() { g(o); return 'x'; } });", false);
    natives(
        "f.hasOwnProperty({ toString() { external(o); return 'x'; } });",
        false,
    );
}

#[test]
fn anonymous_native_keeps_its_model() {
    check(
        "const f = Object.getOwnPropertyDescriptor(unknown(), 'x').get; f.zap(o);",
        false,
    );
}

#[test]
fn direct_namespace_form_retains_hir_dispatch() {
    // member_tail.rs:129 collapses direct static-call receivers to the
    // intrinsic sentinel; an undescribed receiver keeps today's path.
    check("Math.zzNoSuchMember(o);", false);
    check("Object.zzNoSuchMember(o);", false);
}

#[test]
fn stored_members_keep_the_receiver_as_this() {
    check(
        "N.arg = o; N.zap = function() { g(this.arg); }; N.zap();",
        false,
    );
    natives(
        "f.arg = o; f.zap = function() { g(this.arg); }; f.zap();",
        false,
    );
}
