//! `this.items.push(x)` on a receiver typed an array lowers to the
//! runtime's append (`array.push_single`): an element store into the
//! receiver, no unknown code. Neither the array nor the pushed value
//! escapes; what the array is later given to still decides.

use super::*;

fn verdict(tail: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        class Bag {{
            items: Svc[] = [];
            add(s: Svc) {{ this.items.push(s); }}
        }}
        function g(a: any) {{ return a.size; }}
        const b = new Bag();
        b.add(new Svc());
        {tail}
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: prints `3`.
#[test]
fn a_pushed_value_stays_known() {
    let v = verdict("console.log(g(b.items[0]));");
    assert!(proven(&v), "{v:?}");
}

/// node (`hostTake = (xs) => { xs[0].size = 'x'; }`): prints `x`.
#[test]
fn an_array_given_to_unknown_code_still_escapes() {
    let v = verdict("(globalThis as any).hostTake(b.items); console.log(g(b.items[0]));");
    assert!(!proven(&v), "{v:?}");
}
