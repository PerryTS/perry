//! The HIR global sentinel (`BuiltinNs::Other`, the receiver a bare global
//! name such as `console` or `setTimeout` is read off) is the global object:
//! its chain is Object, not Function (node: `typeof globalThis` is
//! "object", not callable). No builtin function value may be it: an Any
//! (`/a/.source`, which may be any builtin function) handed to unknown code
//! no longer reaches the sentinel's fields, so `console.log` is not an
//! unknown callee that takes every argument with it. Handed to unknown code
//! itself, the sentinel is the global object escaping, as `globalThis` is.
//! Fixtures from /root/SENTINEL2/fx (s1_timer, s3_recv; controls c0_global,
//! c3_consolehook).
use super::*;

/// The read sites under `key` on the line holding `marker`.
fn at<'a>(pf: &'a ProgramFacts, src: &str, marker: &str, key: &str) -> Vec<&'a SiteFacts> {
    let m = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker {marker}"));
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `{key}` site on the {marker} line");
    s
}

fn verdicts<'a>(pf: &'a ProgramFacts, src: &str, key: &str) -> Vec<&'a Verdict> {
    at(pf, src, "// SITE", key)
        .iter()
        .map(|s| &s.verdict)
        .collect()
}

/// Is every `o.k.m` read on the `// SITE` line proven (`want`) or unproven?
fn assert_site(src: &str, want: bool) {
    let pf = facts(src);
    for key in ["k", "m"] {
        let v = verdicts(&pf, src, key);
        assert!(
            v.iter().all(|v| matches!(v, Verdict::No(_)) != want),
            "{key}: {v:?}"
        );
    }
}

const K: &str = "class K { m(): number { return 1; } }\n\
    const o: any = { k: new K() };\n";

/// s1_timer: an Any handed to `setTimeout` reaches unknown code. It may be
/// any builtin function, never the global object, so `console` read off
/// the sentinel is not ⊤ and `console.log(o)` does not hand `o` over.
#[test]
fn an_escaped_builtin_function_is_not_the_global_sentinel() {
    let src = format!(
        "{K}const g: any = /a/.source;\n\
         setTimeout(g, 0);\n\
         console.log(o);\n\
         console.log(o.k.m()); // SITE\n"
    );
    assert_site(&src, true);
}

/// s3_recv: the Any as the receiver of an unknown call (`g.call(g)`).
#[test]
fn an_any_receiver_of_an_unknown_call_leaves_console_modeled() {
    let src = format!(
        "{K}const g: any = /a/.source;\n\
         g.call(g);\n\
         console.log(o);\n\
         console.log(o.k.m()); // SITE\n"
    );
    assert_site(&src, true);
}

/// Control c0_global: the global object handed to a function read off it
/// under a computed key. Unknown code holds the global object, so what was
/// stored there is unknown.
#[test]
fn the_global_object_handed_to_unknown_code_stays_unproven() {
    let src = format!(
        "{K}(globalThis as any).box = o;\n\
         const u: any = (globalThis as any)[process.argv[2]];\n\
         u(globalThis);\n\
         const b: any = (globalThis as any).box;\n\
         console.log(b.k.m()); // SITE\n"
    );
    assert_site(&src, false);
}

/// Control c3_consolehook: `console` replaced by an object whose `log` is a
/// computed global read; `console.log(1)` runs unknown code, which may
/// rewrite the global `box`.
#[test]
fn a_replaced_console_log_stays_unknown_code() {
    let src = format!(
        "{K}(globalThis as any).box = o;\n\
         (globalThis as any).console = {{ log: (globalThis as any)[process.argv[2]] }};\n\
         console.log(1);\n\
         const b: any = (globalThis as any).box;\n\
         console.log(b.k.m()); // SITE\n"
    );
    assert_site(&src, false);
}
