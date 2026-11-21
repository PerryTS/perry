//! CALLREGION: a call the solver makes synchronously for a builtin call
//! site (`f.call`, `f.apply`, a bound call, an array callback) throws into
//! that site's region (ECMA-262 7.3.14 Call, 14.15 try), not into the
//! program-wide thrown cell every catch reads.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_async_to_generator(&mut hir);
    perry_transform::transform_generators(&mut hir);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// The kind of `Caught.value`, which holds what the catch under test saw.
struct Seen {
    top: bool,
    mask: u64,
}

impl std::fmt::Debug for Seen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "top={} mask={:#x}", self.top, self.mask)
    }
}

impl Seen {
    fn mask(&self) -> u64 {
        self.mask
    }
}

fn caught(src: &str) -> (Seen, String) {
    let pf = facts_of(src);
    let k = &field(tmpl(&pf, "Caught"), "value").kind;
    let seen = Seen {
        top: k.top,
        mask: k.mask() as u64,
    };
    (seen, pf.dump_text())
}

const CAUGHT: &str = "class Caught { value: any = null; }
    const box = new Caught();\n";

/// RED: `unlock.call(..)` of a function that rethrows what it caught
/// (a string): the rethrow reaches the catch around `.call` only, never
/// the catch of an unrelated function, which sees only its own number.
#[test]
fn callregion_call_rethrow_stays_in_its_try() {
    let src = format!(
        "{CAUGHT}
        function unlock(this: any, s: string) {{
            try {{ if (s === \"y\") throw \"s\"; }} catch (e) {{ throw e; }}
        }}
        function safe() {{ try {{ unlock.call(null, process.argv[2]); }} catch (e) {{ console.log(\"no\"); }} }}
        function k(s: string) {{ if (s === \"x\") throw 7; }}
        function h() {{ try {{ k(process.argv[2]); }} catch (e: any) {{ box.value = e; }} }}
        safe();
        h();"
    );
    let (k, dump) = caught(&src);
    assert!(
        !k.top && k.mask() & 1 != 0 && k.mask() & 2 == 0,
        "{k:?}\n{dump}"
    );
}

/// KEEP: a throw inside a callback `Array.prototype.map` calls, inside a
/// try, reaches that try's catch.
#[test]
fn callregion_map_callback_throw_reaches_catch() {
    let src = format!(
        "{CAUGHT}
        function h(s: string) {{
            try {{ [1, 2].map((x) => {{ if (s === \"x\") throw \"t\"; return x; }}); }}
            catch (e: any) {{ box.value = e; }}
        }}
        h(process.argv[2]);"
    );
    let (k, dump) = caught(&src);
    assert!(k.top || k.mask() & 2 != 0, "{k:?}\n{dump}");
}

/// KEEP: a throw two levels down reaches the catch.
#[test]
fn callregion_two_levels_down_reaches_catch() {
    let src = format!(
        "{CAUGHT}
        function a(s: string) {{ if (s === \"x\") throw \"t\"; }}
        function b(s: string) {{ a(s); }}
        function h() {{ try {{ b(process.argv[2]); }} catch (e: any) {{ box.value = e; }} }}
        h();"
    );
    let (k, dump) = caught(&src);
    assert!(k.top || k.mask() & 2 != 0, "{k:?}\n{dump}");
}

/// KEEP: what `g.call`, `g.apply` and a bound `g` throw reaches the catch
/// around each call.
#[test]
fn callregion_call_apply_bound_reach_their_catch() {
    for call in ["g.call(null, s)", "g.apply(null, [s])", "g.bind(null)(s)"] {
        let src = format!(
            "{CAUGHT}
            function g(this: any, s: string) {{ if (s === \"x\") throw \"t\"; }}
            function h(s: string) {{ try {{ {call}; }} catch (e: any) {{ box.value = e; }} }}
            h(process.argv[2]);"
        );
        let (k, dump) = caught(&src);
        assert!(k.top || k.mask() & 2 != 0, "{call}: {k:?}\n{dump}");
    }
}
