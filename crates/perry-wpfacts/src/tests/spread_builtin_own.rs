//! HUB14 SPREADOWN: a copy from a builtin function value copies its own
//! enumerable keys only (ECMA-262 7.3.25 CopyDataProperties): a member a
//! program stored on Function.prototype is inherited, never copied.
//! perry-web css/anim.ts:144 `{ ...s.values }` copied Function.prototype /
//! Object.prototype values (`constructor` = Object, its statics next).
use super::*;

fn field_proven(src: &str) -> bool {
    let end = src.find("// OBSERVE").expect("site");
    let start = src[..end].rfind('\n').map_or(0, |n| n + 1);
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "field" && (start as u32..end as u32).contains(&s.byte_offset))
        .collect();
    assert_eq!(sites.len(), 1, "{src}\n{}", pf.dump_text());
    !matches!(sites[0].verdict, Verdict::No(_))
}

fn src(store: &str, copy: &str, call: &str) -> String {
    format!(
        r#"
declare function external(x: any): void;
const o: any = {{ field: 1 }};
function g(p: any) {{ p.field = 2; external(p); }}
{store}
const c: any = {copy};
{call}
console.log(o.field); // OBSERVE
"#
    )
}

#[test]
fn copy_of_builtin_function_takes_no_inherited_member() {
    let proto = "(Function.prototype as any).zap = g;";
    // control: no call of the copy's member
    assert!(field_proven(&src(proto, "{ ...Math.max }", "")));
    // inherited: `{ ...Math.max }` has no own `zap`
    for copy in ["{ ...Math.max }", "Object.assign({}, Math.max)"] {
        assert!(field_proven(&src(proto, copy, "if (typeof c.zap === \"function\") c.zap(o);")), "{copy}");
    }
    // own (a program store onto the function value is enumerable): copied
    let own = "(Math.max as any).zap = g;";
    for copy in ["{ ...Math.max }", "Object.assign({}, Math.max)"] {
        assert!(!field_proven(&src(own, copy, "if (typeof c.zap === \"function\") c.zap(o);")), "{copy}");
    }
}

// The leak seen at perry-web css/anim.ts:144: the copy read the REALM values
// of the source (its own statics, Function.prototype's, Object.prototype's).
// None is an own enumerable key, so none reaches the copy.
const COPIES_OBJ: [&str; 2] = ["{ ...Object }", "Object.assign({}, Object)"];

#[test]
fn copy_of_builtin_takes_no_realm_static_named() {
    for copy in COPIES_OBJ {
        let call = "if (typeof c.defineProperty === \"function\") c.defineProperty(o, \"field\", { get() { return 3; } });";
        assert!(field_proven(&src("", copy, call)), "{copy}");
    }
}

#[test]
fn copy_of_builtin_takes_no_realm_static_any_key() {
    for copy in COPIES_OBJ {
        let call = "declare const kk: string;\nconst f: any = c[kk];\nif (typeof f === \"function\") f(o, \"field\", { get() { return 3; } });";
        assert!(field_proven(&src("", copy, call)), "{copy}");
    }
}

#[test]
fn copy_of_builtin_takes_no_function_prototype_member() {
    for copy in ["{ ...parseFloat }", "Object.assign({}, parseFloat)"] {
        let call = "const k: any = c.call;\nif (typeof k === \"function\") k.call(external, null, o);";
        assert!(field_proven(&src("", copy, call)), "{copy}");
    }
}

// What the copy HOLDS: a read through the copy names no realm value of the
// source. `c.call` on a literal copy: the copy's own chain (Object.prototype)
// has no `call`, and Function.prototype.call is inherited by the source, never
// an own key of it. Red before SPREADOWN: the copy read the realm chain
// values of every builtin the source may denote (Function.prototype's).
fn witness_receivers(src: &str) -> Vec<String> {
    let pf = facts(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| !s.is_call && s.key == "witness").collect();
    assert_eq!(sites.len(), 1, "{src}\n{}", pf.dump_text());
    let mut r: Vec<String> = sites[0].receiver_values.iter().map(|v| v.display.clone()).collect();
    r.extend(sites[0].receivers.iter().cloned());
    if sites[0].top {
        r.push("TOP".into());
    }
    r
}

#[test]
fn copy_of_builtin_holds_no_realm_value() {
    // `({} as any)[kk]` may be any method of Object.prototype (one value for
    // all of them), so the copy's source overlaps each named one: still only
    // their own enumerable keys are copied, never Function.prototype's
    // members (`call`), which they inherit.
    let mut bad = Vec::new();
    let pre = "const kk: string = process.argv[2];\nconst pv: any = Object.prototype.valueOf;\nconst ph: any = Object.prototype.hasOwnProperty;\nconsole.log(pv, ph);\n";
    for src in [
        "parseFloat",
        "({} as any)[kk]",
        "(Promise as any)[kk]",
        "(\"s\" as any)[kk]",
        "(Reflect as any)[kk]",
        "(Promise.resolve(1) as any)[kk]",
    ] {
        let ctl = format!("{pre}const c: any = {src};\nconst v: any = c;\nconsole.log(v.witness);\n");
        assert!(!witness_receivers(&ctl).is_empty(), "control {src}");
        for copy in [format!("{{ ...{src} }}"), format!("Object.assign({{}}, {src})")] {
            let s = format!("{pre}const c: any = {copy};\nconst v: any = c.call;\nconsole.log(v.witness);\n");
            if !witness_receivers(&s).is_empty() {
                bad.push(copy);
            }
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}
