//! PERFNOW34 part 2, rule T7: `x !== Object.prototype` true (`===` false)
//! removes `%Object.prototype%` (`ObjectProto`, exact since 2d5dde97c2) from
//! local `x` (`Cons::NotObjectProto`). The shape of the WebGL hook walk: the
//! own methods of every prototype of `s` above `Object.prototype` are
//! applied to `s` (in a function body: a module body has no narrowing
//! scope, `gen::walk::walk_module`). With `Object.prototype` in `p`, `f` holds
//! `__defineGetter__` and friends (annex B), which install an accessor on
//! `s`, so `s.size` is no own data read; the test keeps them out.
use super::*;

fn verdict(walk: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; m(x: any) {{ return x; }} }}
        function g(a: any) {{ return a.size; }}
        function main() {{
        const s: any = new Svc();
        {walk} {{
          for (const k of Object.getOwnPropertyNames(p)) {{
            if (k === "constructor") continue;
            const d: any = Object.getOwnPropertyDescriptor(p, k);
            if (d === undefined || typeof d.value !== "function") continue;
            const f: any = d.value;
            f.apply(s, ["size", () => 5]);
          }}
          {step}
        }}
        console.log(g(s));
        }}
        main();
        "#,
        walk = walk,
        step = if walk.starts_with("for") {
            ""
        } else {
            "p = Object.getPrototypeOf(p);"
        },
    );
    site(&facts(&src), "size").verdict.clone()
}

/// `while` (narrowed before part 1): rule T7 alone.
#[test]
fn while_not_object_prototype() {
    let v = verdict(
        "let p: any = Object.getPrototypeOf(s); while (p !== null && p !== Object.prototype)",
    );
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// `==` false: the same (7.2.15 step 1).
#[test]
fn while_loose_not_object_prototype() {
    let v = verdict(
        "let p: any = Object.getPrototypeOf(s); while (!(p == null || p == Object.prototype))",
    );
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// The hooks loop itself (part 1 + rule T7): `p` never holds
/// `Object.prototype` in the body.
#[test]
fn for_hooks_walk_not_object_prototype() {
    let v = verdict(
        "for (let p: any = Object.getPrototypeOf(s); p !== null && p !== Object.prototype; p = Object.getPrototypeOf(p))",
    );
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// Control: another prototype excludes nothing of `Object.prototype`.
#[test]
fn while_array_prototype_keeps_object_prototype() {
    let v = verdict(
        "let p: any = Object.getPrototypeOf(s); while (p !== null && p !== Array.prototype)",
    );
    assert!(matches!(v, Verdict::No(_)), "{v:?}");
}
