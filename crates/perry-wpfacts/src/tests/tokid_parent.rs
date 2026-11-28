//! TOKID1: `class X extends globalThis.C` and `class X extends C` evaluate
//! the same realm intrinsic %C% (ECMA-262 19.1: the global object's own
//! property `C` is the constructor; a bare `C` resolves through the global
//! environment's object record to that property, 9.1.1.2.6 GetBindingValue).
//! One value, one token: what the solve proves for one form it proves for
//! the other. Red on base: the member read gave the namespace token, so
//! `super()` saw a value other than the declared constructor (E21 + the
//! super-builtin-construct ⊤, apply.rs SuperCtorParent).
use super::*;

fn site(pf: &ProgramFacts, src: &str) -> (bool, String) {
    let m = src.find("// SITE").expect("marker");
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == "x" && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no x site");
    (
        s.iter().all(|s| !matches!(s.verdict, Verdict::No(_))),
        format!(
            "{:?}",
            s.iter()
                .map(|s| (&s.receivers, &s.verdict))
                .collect::<Vec<_>>()
        ),
    )
}

fn error_program(parent: &str) -> String {
    format!(
        "class HttpError extends {parent} {{ r: any; constructor(m: string, r: any) {{ super(m); this.r = r; }} }}\n\
         class V {{ x: number; constructor(x: number) {{ this.x = x; }} add(o: V): V {{ return new V(this.x + o.x); }} }}\n\
         const a = new V(1), b = new V(2);\n\
         function f(m: string): any {{ try {{ throw new HttpError(m, a); }} catch (e: any) {{ return e.message; }} }}\n\
         console.log(a.add(b).x, f(\"q\")); // SITE\n"
    )
}

fn map_program(parent: &str) -> String {
    format!(
        "class Reg extends {parent} {{ r: any; constructor(r: any) {{ super(); this.r = r; }} }}\n\
         class V {{ x: number; constructor(x: number) {{ this.x = x; }} add(o: V): V {{ return new V(this.x + o.x); }} }}\n\
         const a = new V(1), b = new V(2);\n\
         const g = new Reg(a); g.set(\"k\", 1);\n\
         console.log(a.add(b).x, g.size, g.get(\"k\")); // SITE\n"
    )
}

/// Control (green on base): the bare name.
#[test]
fn tokid_bare_error_parent_is_proven() {
    let src = error_program("Error");
    let (ok, v) = site(&facts(&src), &src);
    assert!(ok, "{v}");
}

/// Expected RED on base: the member form of the same intrinsic.
#[test]
fn tokid_global_member_error_parent_is_proven() {
    let src = error_program("globalThis.Error");
    let (ok, v) = site(&facts(&src), &src);
    assert!(ok, "{v}");
}

/// The two forms give one verdict for another realm constructor (`Map`
/// declares no construct effects: unproven in both forms, E21).
#[test]
fn tokid_global_member_map_parent_equals_bare() {
    let bare = map_program("Map");
    let member = map_program("globalThis.Map");
    let (ok_b, v_b) = site(&facts(&bare), &bare);
    let (ok_m, v_m) = site(&facts(&member), &member);
    assert_eq!(ok_b, ok_m, "bare {v_b}\nmember {v_m}");
}

/// A native error constructor through another name of the global object.
#[test]
fn tokid_window_member_type_error_parent_is_proven() {
    let src = error_program("window.TypeError");
    let (ok, v) = site(&facts(&src), &src);
    assert!(ok, "{v}");
}
