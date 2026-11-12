//! A builtin function value is the native it was read from: a call
//! through the value (an element call `targets[i]()`, `f.call(x)`) runs that
//! native's own model with the receiver the call gives, as a call of the
//! member by name does. A native the solver cannot name, or one the
//! receiver's family does not own, stays an unknown callee.

use super::*;

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// The verdict of the one `sv` read (in `g`).
fn sv(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Sym {{ sv: number; constructor(sv: number) {{ this.sv = sv; }} }}
        function g(o: any) {{ return o.sv; }}
        {body}
        "#
    );
    site(&facts(&src), "sv").verdict.clone()
}

/// node: prints `1`. The element call binds `this` to the array:
/// `slice` reads its elements into a fresh array; nothing escapes, so the
/// `Sym` the array holds keeps its field.
#[test]
fn an_element_call_of_a_native_runs_its_model() {
    let v = sv(r#"
        const s = new Sym(1);
        const targets: any[] = [() => 1, [].slice, s];
        function run(i: number) { return targets[i](); }
        run(0);
        run(1);
        console.log(g(s));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`. `Function.prototype.call` of the native: `slice` runs
/// with the array given as its receiver.
#[test]
fn call_of_a_native_value_runs_its_model() {
    let v = sv(r#"
        const fs: any[] = [[].slice];
        const arr = [new Sym(1)];
        const out = fs[0].call(arr);
        console.log(g(out[0]));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node (sabotage, `hook = () => {}`): prints `1`. The array the member is
/// read from has its own `slice`, a function of the program's that hands
/// `this` to code the analysis cannot see: the value read is that function.
/// (Replacing `Array.prototype.slice` itself is refused by the runtime,
/// DEC 250: the read stays the native.)
#[test]
fn a_shadowed_member_stays_unknown() {
    let v = sv(r#"
        const a2: any = [];
        a2.slice = function (this: any) {
            (globalThis as any).hook(this);
            return [];
        };
        const fs: any[] = [a2.slice];
        const arr = [new Sym(1)];
        fs[0].call(arr);
        console.log(g(arr[0]));
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// node: prints `1`. A computed read of an array names one of the members
/// its chain describes (or none: `undefined`, the call throws); the solver
/// names each and runs its model with the array as `this`, and none of them
/// hands an element to unknown code.
#[test]
fn a_native_under_a_computed_key_runs_the_named_members() {
    let v = sv(r#"
        const k = String(Math.random());
        const fs: any[] = [([] as any)[k]];
        const arr = [new Sym(1)];
        try { fs[0].call(arr); } catch (e) {}
        console.log(g(arr[0]));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// Control: a function from unknown code (a module outside the program,
/// run at load) is a callee the solver cannot name: called as before (the
/// receiver escapes).
#[test]
fn a_function_from_unknown_code_stays_unknown() {
    let v = sv(r#"
        import "unseen-host";
        const fs: any[] = [(globalThis as any).unseenFn];
        const arr = [new Sym(1)];
        try { fs[0].call(arr); } catch (e) {}
        console.log(g(arr[0]));
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// node: prints `1`. A native another family owns, called on an array
/// (`Map.prototype.get` on an array throws): no model of it runs on the
/// array, which escapes as before.
#[test]
fn a_native_of_another_family_stays_unknown() {
    let v = sv(r#"
        const fs: any[] = [new Map().get];
        const arr = [new Sym(1)];
        try { fs[0].call(arr); } catch (e) {}
        console.log(g(arr[0]));
    "#);
    assert!(!proven(&v), "{v:?}");
}
